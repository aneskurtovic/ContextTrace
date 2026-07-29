export function formatTokens(value: number | null | undefined): string {
  if (value == null) return "—";
  if (value >= 1_000_000) return `${(value / 1_000_000).toFixed(2)}m`;
  if (value >= 1_000) {
    const precision = value >= 100_000 ? 0 : 1;
    return `${(value / 1_000).toFixed(precision)}k`;
  }
  return value.toLocaleString();
}

export function formatBytes(value: number): string {
  if (value >= 1024 ** 3) return `${(value / 1024 ** 3).toFixed(1)} GB`;
  if (value >= 1024 ** 2) return `${(value / 1024 ** 2).toFixed(1)} MB`;
  if (value >= 1024) return `${(value / 1024).toFixed(1)} KB`;
  return `${value} B`;
}

export function formatPercent(value: number | null | undefined): string {
  if (value == null) return "—";
  return `${Math.round(value * 100)}%`;
}

export function formatActivity(value: string | null): string {
  if (!value) return "Unknown";
  const date = new Date(value);
  const now = Date.now();
  const days = Math.floor((now - date.getTime()) / 86_400_000);
  if (days <= 0) {
    return date.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
  }
  if (days === 1) return "Yesterday";
  if (days < 7) return `${days} days ago`;
  return date.toLocaleDateString([], { month: "short", day: "numeric" });
}

export function projectName(project: string | null): string {
  if (!project) return "Unknown project";
  const trimmed = project.replace(/[\\/]+$/, "");
  return trimmed.split(/[\\/]/).pop() || project;
}

export function shortId(id: string): string {
  return id.length > 12 ? id.slice(0, 8) : id;
}

export function errorMessage(error: unknown): string {
  if (error instanceof Error) return error.message;
  return String(error);
}
