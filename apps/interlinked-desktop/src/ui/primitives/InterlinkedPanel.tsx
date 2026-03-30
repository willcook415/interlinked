import type { HTMLAttributes, ReactNode } from "react";
import { cx } from "./cx";

export type InterlinkedPanelHeaderProps = {
  title: string;
  description?: string;
  meta?: ReactNode;
  className?: string;
};

export function InterlinkedPanelHeader({
  title,
  description,
  meta,
  className,
}: InterlinkedPanelHeaderProps) {
  return (
    <div className={cx("il-panel-header", className)}>
      <div className="il-panel-header-text">
        <h2 className="il-type-title">{title}</h2>
        {description ? <p className="il-type-body">{description}</p> : null}
      </div>
      {meta ? <div className="il-panel-header-meta">{meta}</div> : null}
    </div>
  );
}

export type InterlinkedSectionPanelProps = HTMLAttributes<HTMLElement> & {
  children: ReactNode;
};

export default function InterlinkedSectionPanel({
  children,
  className,
  ...rest
}: InterlinkedSectionPanelProps) {
  return (
    <section className={cx("il-section-panel", className)} {...rest}>
      {children}
    </section>
  );
}
