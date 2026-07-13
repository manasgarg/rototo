// Build-time composition (design/console-surfaces.md "What an extension
// is"): this deployment's console build lists its installed experiences
// here, and nowhere else. The two in-repo extensions are reference
// implementations with zero privilege; a deployment adds its own by adding
// an import. Everything else about extensions — what they may read, what
// they may propose — lives in the contract (src/extension-api.ts).

import type { ExperienceModule } from "@/extension-api.ts";
import flags from "@/extensions/flags/index.tsx";
import table from "@/extensions/table/index.tsx";

export const EXPERIENCES: ExperienceModule[] = [table, flags];

export function experienceFor(kind: string | null): ExperienceModule | null {
    if (kind === null) {
        return null;
    }
    return EXPERIENCES.find((experience) => experience.kind === kind) ?? null;
}
