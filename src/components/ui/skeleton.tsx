import { cn } from "@/lib/utils";

function Skeleton({ className, ...props }: React.ComponentProps<"div">) {
  return <div className={cn("animate-pulse-soft rounded-sm bg-sunken", className)} {...props} />;
}

export { Skeleton };
