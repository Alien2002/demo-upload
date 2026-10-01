// src/components/ImportWizard/ImportWizardModal.tsx
//
// Four steps: workspace -> folder -> file (read into staging) -> columns.
// The table is only created at the end of step 4. Going Back from step 4 or
// closing the wizard discards the staged data.
import { useRef, useState } from "react";

import { useWorkspaceStore } from "../../stores/useWorkspaceStore";
import { useFolderStore } from "../../stores/useFolderStore";
import { useTableStore } from "../../stores/useTableStore";
import { api, type StagedImport, type TableMeta } from "../../lib/api";
import { Modal } from "../common/Modal";
import { Step1_SelectWorkspace } from "./Step1_SelectWorkspace";
import { Step2_SelectFolder } from "./Step2_SelectFolder";
import { Step3_ImportFile } from "./Step3_ImportFile";
import { Step4_Columns } from "./Step4_Columns";

type Props = { onClose: () => void };
type Step = 1 | 2 | 3 | 4;

const TITLES = ["Workspace", "Folder", "File", "Columns"];

export function ImportWizardModal({ onClose }: Props) {
  const [step, setStep] = useState<Step>(1);
  const [workspaceId, setWorkspaceId] = useState<string | null>(
    useWorkspaceStore.getState().current_workspace_id,
  );
  const [folderId, setFolderId] = useState<string | null>(() => {
    const fs = useFolderStore.getState();
    return fs.workspace_id === workspaceId ? fs.current_folder_id : null;
  });
  const [staged, setStaged] = useState<StagedImport | null>(null);
  const [running, setRunning] = useState(false);

  // Where the staged data lives, for discarding it later.
  const stagedAt = useRef<{ ws: string; folder: string; id: string } | null>(null);

  function discardStaged() {
    const s = stagedAt.current;
    stagedAt.current = null;
    setStaged(null);
    if (s) void api.tables.discard(s.ws, s.folder, s.id).catch(() => {});
  }

  function close() {
    discardStaged();
    onClose();
  }

  function chooseWorkspace(id: string) {
    if (id !== workspaceId) setFolderId(null);
    setWorkspaceId(id);
  }

  function handleStaged(s: StagedImport) {
    if (!workspaceId || !folderId) return;
    stagedAt.current = { ws: workspaceId, folder: folderId, id: s.staging_id };
    setStaged(s);
    setStep(4);
  }

  async function handleImported(meta: TableMeta) {
    stagedAt.current = null; // committed: nothing to discard
    if (!workspaceId || !folderId) return;
    useWorkspaceStore.getState().setCurrentWorkspace(workspaceId);
    useFolderStore.getState().selectFolderIn(workspaceId, folderId);
    await useTableStore.getState().openTableIn(workspaceId, folderId, meta.table);
    onClose();
  }

  return (
    <Modal
      title={`Import — ${TITLES[step - 1]}`}
      onClose={close}
      locked={running}
      width={step === 4 ? 1040 : 560}
    >
      <ol className="wizard-steps">
        {TITLES.map((t, i) => (
          <li key={t} className={i + 1 === step ? "current" : i + 1 < step ? "done" : ""}>
            {i + 1}. {t}
          </li>
        ))}
      </ol>

      {step === 1 && (
        <Step1_SelectWorkspace
          selected={workspaceId}
          onSelect={chooseWorkspace}
          onNext={() => setStep(2)}
          onCancel={close}
        />
      )}
      {step === 2 && workspaceId && (
        <Step2_SelectFolder
          workspaceId={workspaceId}
          selected={folderId}
          onSelect={setFolderId}
          onBack={() => setStep(1)}
          onNext={() => setStep(3)}
        />
      )}
      {step === 3 && workspaceId && folderId && (
        <Step3_ImportFile
          workspaceId={workspaceId}
          folderId={folderId}
          onBack={() => setStep(2)}
          onClose={close}
          onRunningChange={setRunning}
          onStaged={handleStaged}
        />
      )}
      {step === 4 && workspaceId && folderId && staged && (
        <Step4_Columns
          workspaceId={workspaceId}
          folderId={folderId}
          staged={staged}
          onBack={() => {
            discardStaged();
            setStep(3);
          }}
          onRunningChange={setRunning}
          onImported={handleImported}
        />
      )}
    </Modal>
  );
}
