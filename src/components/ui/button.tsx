import type * as React from "react";
import { Slot } from "@radix-ui/react-slot";
import { cva, type VariantProps } from "class-variance-authority";
import { cn } from "@/lib/utils";

const buttonVariants = cva(
  "inline-flex shrink-0 items-center justify-center gap-2 whitespace-nowrap rounded-md text-sm font-medium outline-none transition-[background-color,color,box-shadow,transform] duration-150 ease-instrument focus-visible:ring-2 focus-visible:ring-brand/35 focus-visible:ring-offset-1 focus-visible:ring-offset-paper disabled:pointer-events-none disabled:opacity-45 [&_svg]:pointer-events-none [&_svg]:shrink-0 [&_svg:not([class*='size-'])]:size-4 active:translate-y-px",
  {
    variants: {
      variant: {
        default: "bg-brand text-brand-ink hover:bg-brand/90 shadow-card",
        ember: "bg-ember text-ember-ink hover:bg-ember/90 shadow-card",
        outline:
          "bg-surface text-ink shadow-hairline hover:bg-raised hover:shadow-[inset_0_0_0_1px_var(--line-strong)]",
        secondary: "bg-sunken text-ink hover:bg-sunken/70",
        ghost: "text-ink-soft hover:bg-sunken hover:text-ink",
        subtle: "bg-brand-tint text-brand hover:bg-brand-tint-strong",
        destructive: "bg-danger text-white hover:bg-danger/90 shadow-card",
        link: "text-brand underline-offset-4 hover:underline",
      },
      size: {
        sm: "h-8 px-2.5 text-[13px]",
        default: "h-9 px-3.5",
        lg: "h-10 px-5 text-[15px]",
        icon: "size-9",
        "icon-sm": "size-8 rounded-sm",
      },
    },
    defaultVariants: {
      variant: "default",
      size: "default",
    },
  },
);

export interface ButtonProps
  extends React.ComponentProps<"button">,
    VariantProps<typeof buttonVariants> {
  asChild?: boolean;
}

function Button({ className, variant, size, asChild = false, ...props }: ButtonProps) {
  const Comp = asChild ? Slot : "button";
  return (
    <Comp
      data-slot="button"
      className={cn(buttonVariants({ variant, size }), className)}
      {...props}
    />
  );
}

export { Button, buttonVariants };
