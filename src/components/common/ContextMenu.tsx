// src/components/common/ContextMenu.tsx
// Right-click menu positioned at the cursor. Closes on outside click or Escape.
import { useEffect } from "react";

export type MenuItem = {
  label: string;
  onClick?: () => void;
  danger?: boolean;
  disabled?: boolean;
  hint?: string;
};

type Props = { x: number; y: number; items: MenuItem[]; onClose: () => void };

export function ContextMenu({ x, y, items, onClose }: Props) {
  useEffect(() => {
    const close = () => onClose();
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("mousedown", close);
    window.addEventListener("keydown", onKey);
    window.addEventListener("blur", close);
    return () => {
      window.removeEventListener("mousedown", close);
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("blur", close);
    };
  }, [onClose]);

  return (
    <ul className="context-menu" style={{ left: x, top: y }} onMouseDown={(e) => e.stopPropagation()}>
      {items.map((item) => (
        <li key={item.label}>
          <button
            className={item.danger ? "danger" : ""}
            disabled={item.disabled}
            onClick={() => {
              onClose();
              item.onClick?.();
            }}
          >
            {item.label}
            {item.hint && <span className="hint">{item.hint}</span>}
          </button>
        </li>
      ))}
    </ul>
  );
}
