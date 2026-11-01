// Stub of @better-t-stack/types for the harness: the compatibility helpers as upstream has
// them, and ProjectNameSchema's rules without zod.
export * from "./compatibility";

const rules: Array<[(name: string) => boolean, string]> = [
  [(n) => n.length >= 1, "Project name cannot be empty"],
  [(n) => n.length <= 255, "Project name must be less than 255 characters"],
  [(n) => n === "." || !n.startsWith("."), "Project name cannot start with a dot (except for '.')"],
  [(n) => n === "." || !n.startsWith("-"), "Project name cannot start with a dash"],
  [
    (n) => !["<", ">", ":", '"', "|", "?", "*"].some((c) => n.includes(c)),
    "Project name contains invalid characters",
  ],
  [(n) => n.toLowerCase() !== "node_modules", "Project name is reserved"],
];

export const ProjectNameSchema = {
  safeParse(name: string) {
    const issues = rules.filter(([ok]) => !ok(name)).map(([, message]) => ({ message }));
    return issues.length ? { success: false, error: { issues } } : { success: true, data: name };
  },
};
