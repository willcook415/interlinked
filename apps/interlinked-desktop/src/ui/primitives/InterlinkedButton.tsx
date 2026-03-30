import type { ButtonHTMLAttributes, ReactNode } from "react";
import { cx } from "./cx";

type InterlinkedButtonTone = "primary" | "secondary" | "ghost";
type InterlinkedButtonSize = "md" | "sm";

export type InterlinkedButtonProps = ButtonHTMLAttributes<HTMLButtonElement> & {
  tone?: InterlinkedButtonTone;
  size?: InterlinkedButtonSize;
  icon?: ReactNode;
};

export default function InterlinkedButton({
  tone = "secondary",
  size = "md",
  icon,
  type = "button",
  className,
  children,
  ...rest
}: InterlinkedButtonProps) {
  return (
    <button
      className={cx(
        "il-button",
        "il-motion-interactive",
        tone === "primary" && "is-primary",
        tone === "secondary" && "is-secondary",
        tone === "ghost" && "is-ghost",
        size === "sm" && "is-sm",
        className
      )}
      type={type}
      {...rest}
    >
      <span className="il-button-content">
        {icon ? <span aria-hidden="true">{icon}</span> : null}
        <span>{children}</span>
      </span>
    </button>
  );
}
