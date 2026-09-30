// src/components/ImportWizard/ImportWizardModal.tsx
//
// Three steps: workspace -> folder -> file. Starts with the current
// workspace/folder preselected, so a quick import is Next, Next, Start.
import { useState } from "react";

import { useWorkspaceStore } from "../../stores/useWorkspaceStore";
import { useFolderStore } from "../../stores/useFolderStore";
import { useTableStore } from "../../stores/useTableStore";
import type { TableMeta } from "../../lib/api";
import { Modal } from "../common/Modal";
import { Step1_SelectWorkspace } from "./Step1_SelectWorkspace";
import { Step2_SelectFolder } from "./Step2_SelectFolder";
import { Step3_ImportFile } from "./Step3_ImportFile";

type Props = { onClose: () => void };

const TITLES = ["Choose a workspace", "Choose a folder", "Import a file"];

export function ImportWizardModal({ onClose }: Props) {
  const [step, setStep] = useState<1 | 2 | 3>(1);
  const [workspaceId, setWorkspaceId] = useState<string | null>(
    useWorkspaceStore.getState().current_workspace_id,
  );
  const [folderId, setFolderId] = useState<string | null>(() => {
    const fs = useFolderStore.getState();
    return fs.workspace_id === workspaceId ? fs.current_folder_id : null;
  });
  const [running, setRunning] = useState(false);

  function chooseWorkspace(id: string) {
    if (id !== workspaceId) setFolderId(null);
    setWorkspaceId(id);
  }

  async function handleImported(meta: TableMeta) {
    if (!workspaceId || !folderId) return;
    // Navigate to the new table.
    useWorkspaceStore.getState().setCurrentWorkspace(workspaceId);
    useFolderStore.getState().selectFolderIn(workspaceId, folderId);
    await useTableStore.getState().openTableIn(workspaceId, folderId, meta.table);
    onClose();
  }

  return (
    <Modal title={`Import — step ${step} of 3: ${TITLES[step - 1]}`} onClose={onClose} locked={running} width={560}>
      <ol className="wizard-steps">
        {TITLES.map((t, i) => (
          <li key={t} className={i + 1 === step ? "current" : i + 1 < step ? "done" : ""}>
            {t}
          </li>
        ))}
      </ol>

      {step === 1 && (
        <Step1_SelectWorkspace
          selected={workspaceId}
          onSelect={chooseWorkspace}
          onNext={() => setStep(2)}
          onCancel={onClose}
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
          onClose={onClose}
          onRunningChange={setRunning}
          onImported={handleImported}
        />
      )}
    </Modal>
  );
}
