import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

/** What a request card shows. The two `unchecked` fields were chosen by the asking program (ADR-0040). */
export type AgentCard = {
  id: number;
  program: string;
  programPath: string;
  startedBy: string[];
  /** What the program calls itself. Not checked. */
  says: string | null;
  /** Why it says it asks. Not checked. */
  reason: string | null;
  wants: string;
  how: string;
  duration: string;
  loud: string[];
  seconds: number;
};

export type Decided = "approved" | "not_approved" | "gone";

export const agentRequests = () => invoke<AgentCard[]>("agent_requests");

/** Approve opens the system Touch ID prompt and resolves when it ends; only an accepted prompt allows. */
export const agentDecide = (id: number, approve: boolean) =>
  invoke<Decided>("agent_decide", { id, approve });

export const onAgentRequest = (handler: (card: AgentCard) => void) =>
  listen<AgentCard>("figlock://agent-request", (e) => handler(e.payload));

export const onAgentDone = (handler: (id: number) => void) =>
  listen<number>("figlock://agent-request-done", (e) => handler(e.payload));
