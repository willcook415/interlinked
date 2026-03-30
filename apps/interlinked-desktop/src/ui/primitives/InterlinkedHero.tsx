import type { HTMLAttributes, ReactNode } from "react";
import { cx } from "./cx";

export type InterlinkedHeroProps = HTMLAttributes<HTMLElement> & {
  eyebrow: string;
  title: string;
  subtitle: string;
  children?: ReactNode;
};

export default function InterlinkedHero({
  eyebrow,
  title,
  subtitle,
  children,
  className,
  ...rest
}: InterlinkedHeroProps) {
  return (
    <header className={cx("il-hero", className)} {...rest}>
      <div className="il-hero-content">
        <p className="il-type-eyebrow">{eyebrow}</p>
        <h1 className="il-type-hero">{title}</h1>
        <p className="il-type-body-lg">{subtitle}</p>
        {children}
      </div>
    </header>
  );
}
