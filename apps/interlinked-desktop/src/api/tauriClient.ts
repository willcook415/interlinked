import { invoke } from "@tauri-apps/api/core";

export function invokeTyped<TResult>(
  command: string,
  args?: Record<string, unknown>
): Promise<TResult> {
  return invoke<TResult>(command, args);
}
