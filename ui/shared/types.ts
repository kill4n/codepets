export type Visible = "working"|"idle"|"needs_input"|"delegating"|"waiting_peer"|"gone";
export interface AgentView { key: string; host: string; session_id: string; name: string; cwd: string|null; pid: number|null; job_id: string|null; parent: string|null; permission: [string, string]|null; visible: Visible; waiting_for: string|null; peer: string|null; since_secs: number }
export interface Animation { sheet: string; row: number; frames: number; fps: number; loop: boolean }
export interface Manifest { name: string; frameSize: [number, number]; scale: number; zoom: number; labelH: number; labelOverlap?: number; animations: Record<string, Animation> }
