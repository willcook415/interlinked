import type { HTMLAttributes, ReactNode } from "react";
import { cx } from "./cx";

export type InterlinkedPageShellProps = HTMLAttributes<HTMLElement> & {
  children: ReactNode;
  centered?: boolean;
};

export default function InterlinkedPageShell({
  children,
  centered = false,
  className,
  ...rest
}: InterlinkedPageShellProps) {
  return (
    <main className={cx("il-page-shell", centered && "is-centered", className)} {...rest}>
      <div className="il-page-shell-inner">{children}</div>
    </main>
  );
}
