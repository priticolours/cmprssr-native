import * as React from "react";
import { Tooltip } from "radix-ui";
import { cn } from "../lib/utils";

/**
 * Local reimplementation of the UI kit exports this app uses
 * (Text, Button, Badge, Callout, Status, TooltipProvider, Toaster, ErrorBoundaryView).
 * Prop shapes match the original usage; styling uses Tailwind v4 theme tokens
 * defined in src/styles.css.
 */

// ── Text ──────────────────────────────────────────────────────────────

type TextVariant =
  | "heading1"
  | "large-strong"
  | "strong"
  | "regular"
  | "small"
  | "small-strong"
  | "small-mono";

type TextColor = "primary" | "secondary" | "tertiary" | "quaternary" | "accent";

const variantClass: Record<TextVariant, string> = {
  heading1: "text-[22px] leading-7 font-semibold tracking-tight",
  "large-strong": "text-[17px] leading-6 font-semibold",
  strong: "text-[15px] leading-6 font-semibold",
  regular: "text-[15px] leading-6 font-normal",
  small: "text-[13px] leading-5 font-normal",
  "small-strong": "text-small-strong",
  "small-mono": "text-small-mono",
};

const colorClass: Record<TextColor, string> = {
  primary: "text-primary",
  secondary: "text-secondary",
  tertiary: "text-tertiary",
  quaternary: "text-quaternary",
  accent: "text-accent",
};

export interface TextProps extends React.HTMLAttributes<HTMLSpanElement> {
  variant?: TextVariant;
  color?: TextColor;
  truncate?: boolean;
}

export function Text({
  variant = "regular",
  color = "primary",
  truncate = false,
  className,
  ...rest
}: TextProps) {
  return (
    <span
      className={cn(variantClass[variant], colorClass[color], truncate && "truncate", className)}
      {...rest}
    />
  );
}

// ── Button ────────────────────────────────────────────────────────────

type ButtonVariant = "accent" | "transparent" | "filled" | "muted";
type ButtonSize = "large" | "medium" | "small";

const buttonVariants: Record<ButtonVariant, string> = {
  accent: "bg-accent text-accent-contrast hover:brightness-110 active:brightness-95",
  transparent: "text-secondary hover:text-primary hover:bg-control",
  filled: "bg-control text-primary hover:bg-control-active",
  muted: "bg-control-subtle text-secondary hover:bg-control hover:text-primary",
};

const buttonSizes: Record<ButtonSize, string> = {
  large: "h-11 px-5 text-[15px] font-semibold rounded-control",
  medium: "h-9 px-4 text-sm font-semibold rounded-control",
  small: "h-7 px-3 text-[13px] font-medium rounded-control",
};

export interface ButtonProps extends React.ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: ButtonVariant;
  size?: ButtonSize;
}

export function Button({
  variant = "filled",
  size = "medium",
  className,
  type = "button",
  ...rest
}: ButtonProps) {
  return (
    <button
      type={type}
      className={cn(
        "inline-flex items-center justify-center gap-2 whitespace-nowrap transition-colors",
        "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent",
        "disabled:pointer-events-none disabled:opacity-50",
        buttonVariants[variant],
        buttonSizes[size],
        className,
      )}
      {...rest}
    />
  );
}

// ── Badge ─────────────────────────────────────────────────────────────

type BadgeColor = "secondary" | "blue" | "green";

const badgeColors: Record<BadgeColor, string> = {
  secondary: "bg-control-active text-secondary",
  blue: "bg-blue-500/15 text-blue-700 dark:text-blue-400",
  green: "bg-green-500/15 text-green-800 dark:text-green-400",
};

export interface BadgeProps extends React.HTMLAttributes<HTMLSpanElement> {
  color?: BadgeColor;
}

export function Badge({ color = "secondary", className, ...rest }: BadgeProps) {
  return (
    <span
      className={cn(
        "inline-flex items-center rounded-pill px-2.5 py-0.5 text-[12px] font-medium",
        badgeColors[color],
        className,
      )}
      {...rest}
    />
  );
}

// ── Callout ───────────────────────────────────────────────────────────

type CalloutColor = "orange" | "red" | "yellow";

const calloutColors: Record<CalloutColor, string> = {
  orange: "border-orange-500/30 bg-orange-500/10 text-orange-800 dark:text-orange-300",
  red: "border-red-500/30 bg-red-500/10 text-red-800 dark:text-red-300",
  yellow: "border-yellow-500/30 bg-yellow-500/10 text-yellow-900 dark:text-yellow-200",
};

export interface CalloutProps extends React.HTMLAttributes<HTMLDivElement> {
  color: CalloutColor;
}

export function Callout({ color, className, ...rest }: CalloutProps) {
  return (
    <div
      role="note"
      className={cn(
        "rounded-card border px-4 py-3 text-[13px] leading-5",
        calloutColors[color],
        className,
      )}
      {...rest}
    />
  );
}

// ── Status (dev-only indicator pill) ──────────────────────────────────

export interface StatusProps extends React.HTMLAttributes<HTMLSpanElement> {
  variant?: "error" | "info";
}

export function Status({ variant = "info", className, ...rest }: StatusProps) {
  return (
    <span
      className={cn(
        "inline-flex items-center rounded-pill px-2.5 py-1 text-[12px] font-medium",
        variant === "error"
          ? "bg-red-500/15 text-red-700 dark:text-red-400"
          : "bg-control-active text-secondary",
        className,
      )}
      {...rest}
    />
  );
}

// ── TooltipProvider / Toaster ─────────────────────────────────────────

export function TooltipProvider({ children }: { children: React.ReactNode }) {
  return <Tooltip.Provider>{children}</Tooltip.Provider>;
}

/** No-op toaster: nothing in the main flow raises toasts. */
export function Toaster() {
  return <div aria-live="polite" className="pointer-events-none fixed inset-0 z-50" />;
}

// ── ErrorBoundaryView ─────────────────────────────────────────────────

/** Route error view for the TanStack router's errorComponent. */
export function ErrorBoundaryView({ error }: { error: unknown }) {
  return (
    <div className="flex h-full flex-col items-center justify-center gap-2 p-8 text-center">
      <Text variant="heading1">Something went wrong</Text>
      <Text variant="small" color="tertiary">
        {error instanceof Error ? error.message : "An unexpected error occurred."}
      </Text>
    </div>
  );
}
