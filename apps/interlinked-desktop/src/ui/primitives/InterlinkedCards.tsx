import type { ButtonHTMLAttributes, ReactNode } from "react";
import { cx } from "./cx";

export type InterlinkedActionCardProps = ButtonHTMLAttributes<HTMLButtonElement> & {
  title: string;
  description: string;
  meta?: string;
  icon?: ReactNode;
};

export function InterlinkedActionCard({
  title,
  description,
  meta,
  icon,
  type = "button",
  className,
  ...rest
}: InterlinkedActionCardProps) {
  return (
    <button className={cx("il-action-card", "il-motion-interactive", className)} type={type} {...rest}>
      <div className="il-action-card-head">
        <p className="il-action-card-title">{title}</p>
        {icon ? <span aria-hidden="true">{icon}</span> : null}
      </div>
      <p className="il-action-card-desc">{description}</p>
      {meta ? <p className="il-action-card-meta">{meta}</p> : null}
    </button>
  );
}

export type InterlinkedSaveSlotCardProps = ButtonHTMLAttributes<HTMLButtonElement> & {
  title: string;
  subtitle?: string;
  meta?: string;
  empty?: boolean;
};

export function InterlinkedSaveSlotCard({
  title,
  subtitle,
  meta,
  empty = false,
  type = "button",
  className,
  ...rest
}: InterlinkedSaveSlotCardProps) {
  if (empty) {
    return (
      <div className={cx("il-save-slot-card", "is-empty", className)}>
        <p className="il-save-slot-card-title">{title}</p>
      </div>
    );
  }

  return (
    <button
      className={cx("il-save-slot-card", "il-motion-interactive", className)}
      type={type}
      {...rest}
    >
      <p className="il-save-slot-card-title">{title}</p>
      {subtitle ? <p className="il-save-slot-card-subtitle">{subtitle}</p> : null}
      {meta ? <p className="il-save-slot-card-meta">{meta}</p> : null}
    </button>
  );
}
