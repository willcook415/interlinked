import type { HTMLAttributes, ReactNode } from "react";
import { cx } from "./cx";

type InterlinkedMetaPillTone = "neutral" | "brand" | "success" | "warning";

export type InterlinkedMetaPillProps = HTMLAttributes<HTMLSpanElement> & {
  tone?: InterlinkedMetaPillTone;
  children: ReactNode;
};

export default function InterlinkedMetaPill({
  tone = "neutral",
  className,
  children,
  ...rest
}: InterlinkedMetaPillProps) {
  return (
    <span
      className={cx(
        "il-pill",
        tone === "brand" && "is-brand",
        tone === "success" && "is-success",
        tone === "warning" && "is-warning",
        className
      )}
      {...rest}
    >
      {children}
    </span>
  );
}
