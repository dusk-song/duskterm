import { cva } from "class-variance-authority";

export { default as Button } from "./Button.vue";

export const buttonVariants = cva(
  "focus-visible:border-[var(--app-focus-border)] focus-visible:bg-[var(--app-focus-bg)] focus-visible:shadow-[var(--app-focus-shadow)] aria-invalid:ring-destructive/20 dark:aria-invalid:ring-destructive/30 aria-invalid:border-destructive rounded-[8px] border border-transparent bg-clip-padding text-xs font-semibold leading-none active:not-aria-[haspopup]:translate-y-px [&_svg:not([class*=size-])]:size-3.5 group/button inline-flex shrink-0 items-center justify-center whitespace-nowrap shadow-[var(--app-control-shadow)] transition-[background,border-color,color,box-shadow,transform] duration-[var(--app-motion-control)] ease-[var(--app-motion-ease)] outline-none select-none disabled:pointer-events-none disabled:opacity-45 [&_svg]:pointer-events-none [&_svg]:shrink-0",
  {
    variants: {
      variant: {
        default: "border-primary/25 bg-primary text-primary-foreground shadow-[inset_0_1px_0_rgba(255,255,255,0.22),0_1px_1px_rgba(0,0,0,0.22)] hover:bg-primary/90",
        outline:
          "border-border bg-card hover:bg-accent hover:text-foreground aria-expanded:bg-accent aria-expanded:text-foreground shadow-[inset_0_1px_0_rgba(255,255,255,0.04)]",
        secondary:
          "border-border bg-secondary text-secondary-foreground hover:bg-accent aria-expanded:bg-accent aria-expanded:text-accent-foreground",
        ghost:
          "text-muted-foreground hover:bg-accent hover:text-foreground aria-expanded:bg-accent aria-expanded:text-foreground",
        destructive:
          "border-destructive/30 bg-destructive/10 text-destructive hover:bg-destructive/20",
        link: "text-primary underline-offset-4 hover:underline",
      },
      size: {
        default:
          "h-[30px] gap-1.5 px-3 in-data-[slot=button-group]:rounded-[8px] has-data-[icon=inline-end]:pr-2 has-data-[icon=inline-start]:pl-2",
        xs: "h-6 gap-1 rounded-[7px] px-2 text-[11px] in-data-[slot=button-group]:rounded-[8px] [&_svg:not([class*=size-])]:size-3",
        sm: "h-7 gap-1 rounded-[7px] px-2.5 in-data-[slot=button-group]:rounded-[8px]",
        lg: "h-8 gap-1.5 rounded-[8px] px-3.5",
        icon: "size-[30px] rounded-[8px]",
        "icon-xs":
          "size-6 rounded-[7px] [&_svg:not([class*=size-])]:size-3",
        "icon-sm":
          "size-7 rounded-[7px]",
        "icon-lg": "size-8 rounded-[8px]",
      },
    },
    defaultVariants: {
      variant: "default",
      size: "default",
    },
  },
);
