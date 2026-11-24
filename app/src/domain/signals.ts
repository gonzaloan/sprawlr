// What the scan measured on a resource: metrics, alarms, error patterns from logs.
import type { Alarm, ArchNode, LogErrors, Metric } from "./types";

export const metricsOf = (n: ArchNode): Metric[] => ((n.attrs?.metrics as Metric[] | undefined) ?? []).filter((m) => Array.isArray(m.values));

export const alarmsOf = (n: ArchNode): Alarm[] => (n.attrs?.alarms as Alarm[] | undefined) ?? [];

/** The series that best shows activity: traffic first, then utilisation. */
export function primaryMetric(n: ArchNode): Metric | undefined {
  const ms = metricsOf(n);
  const order = ["requests", "invocations", "sent", "published", "writes", "started", "cpu"];
  return order.map((k) => ms.find((m) => m.key === k)).find(Boolean) ?? ms[0];
}

/** Error patterns read from the resource's logs, if the scan read them. */
export function logErrorsOf(n: ArchNode): LogErrors | undefined {
  const l = (n.attrs as Record<string, unknown> | undefined)?.log_errors as LogErrors | undefined;
  return l && Array.isArray(l.patterns) ? l : undefined;
}
