// src/components/common/Modal.tsx
import { useEffect, type ReactNode } from "react";

type Props = {
  title: string;
  onClose: () => void;
  children: ReactNode;
  footer?: ReactNode;
  width?: number;
  /** Block closing (e.g. while an import runs). */
  locked?: boolean;
};

export function Modal({ title, onClose, children, footer, width = 520, locked = false }: Props) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape" && !locked) onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose, locked]);

  return (
    <div className="modal-backdrop" onMouseDown={() => !locked && onClose()}>
      <div className="modal" style={{ width }} onMouseDown={(e) => e.stopPropagation()}>
        <header className="modal-header">
          <h2>{title}</h2>
          <button className="icon-btn" onClick={onClose} disabled={locked} aria-label="Close">
            ✕
          </button>
        </header>
        <div className="modal-body">{children}</div>
        {footer && <footer className="modal-footer">{footer}</footer>}
      </div>
    </div>
  );
}
