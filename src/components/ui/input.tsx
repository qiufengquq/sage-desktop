import type * as React from "react";
import { cn } from "@/lib/utils";

function Input({ className, type, ...props }: React.ComponentProps<"input">) {
  return (
    <input
      type={type}
      data-slot="input"
      className={cn(
        "flex h-9 w-full min-w-0 rounded-md bg-field px-3 py-1 text-sm text-ink shadow-hairline transition-[box-shadow,background-color] outline-none",
        "placeholder:text-ink-muted/80",
        "focus-visible:shadow-[inset_0_0_0_1px_var(--brand)] focus-visible:ring-2 focus-visible:ring-brand/15",
        "disabled:cursor-not-allowed disabled:opacity-50",
        "aria-invalid:shadow-[inset_0_0_0_1px_var(--danger)]",
        className,
      )}
      {...props}
    />
  );
}

function Textarea({ className, ...props }: React.ComponentProps<"textarea">) {
  return (
    <textarea
      data-slot="textarea"
      className={cn(
        "flex min-h-20 w-full rounded-md bg-field px-3 py-2 text-sm text-ink shadow-hairline transition-[box-shadow] outline-none",
        "placeholder:text-ink-muted/80",
        "focus-visible:shadow-[inset_0_0_0_1px_var(--brand)] focus-visible:ring-2 focus-visible:ring-brand/15",
        "disabled:cursor-not-allowed disabled:opacity-50 resize-y",
        className,
      )}
      {...props}
    />
  );
}

export { Input, Textarea };
