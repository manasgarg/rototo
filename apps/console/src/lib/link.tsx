import type { ComponentProps } from "react";
import { Link as RouterLink } from "react-router";

/* The components were written against next/link's `href` prop. This shim
   keeps that surface so navigation stays a one-line import change. */
export function Link({
    href,
    ...rest
}: { href: string } & Omit<ComponentProps<typeof RouterLink>, "to">) {
    return <RouterLink to={href} {...rest} />;
}
