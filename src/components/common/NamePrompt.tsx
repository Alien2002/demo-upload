// src/components/common/NamePrompt.tsx
// Small modal asking for a name (new workspace, new folder).
import { useState } from "react";
import { Modal } from "./Modal";
import { errorText } from "../../lib/api";

type Props = {
  title: string;
  label: string;
  placeholder?: string;
  confirmText?: string;
  onSubmit: (name: string) => Promise<unknown>;
  onClose: () => void;
};

export function NamePrompt({ title, label, placeholder, confirmText = "Create", onSubmit, onClose }: Props) {
  const [name, setName] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function submit() {
    if (!name.trim() || busy) return;
    setBusy(true);
    setError(null);
    try {
      await onSubmit(name.trim());
      onClose();
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <Modal
      title={title}
      onClose={onClose}
      width={400}
      footer={
        <>
          <button onClick={onClose}>Cancel</button>
          <button className="primary" onClick={submit} disabled={!name.trim() || busy}>
            {busy ? "Working…" : confirmText}
          </button>
        </>
      }
    >
      <label className="field">
        <span>{label}</span>
        <input
          autoFocus
          value={name}
          placeholder={placeholder}
          onChange={(e) => setName(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && submit()}
        />
      </label>
      {error && <p className="error-text">{error}</p>}
    </Modal>
  );
}
