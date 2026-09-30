// src/components/ImportButton.tsx
// Header button that opens the import wizard.
import { useState } from "react";
import { ImportWizardModal } from "./ImportWizard/ImportWizardModal";

export function ImportButton() {
  const [open, setOpen] = useState(false);
  return (
    <>
      <button className="primary" onClick={() => setOpen(true)}>
        Import file
      </button>
      {open && <ImportWizardModal onClose={() => setOpen(false)} />}
    </>
  );
}
