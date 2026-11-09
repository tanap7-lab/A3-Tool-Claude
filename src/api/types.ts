// Mirrors crates/a3-core/src/model.rs.

export type Letter = 'A' | 'B' | 'C' | 'D';
export const LETTERS: Letter[] = ['A', 'B', 'C', 'D'];

export const STEP_NAMES: Record<Letter, string> = {
  A: 'Define the problem',
  B: 'Analyze root cause',
  C: 'Implement solution',
  D: 'Validate and standardize',
};

export const STEP_SHORT: Record<Letter, string> = { A: 'Define', B: 'Analyze', C: 'Implement', D: 'Validate' };

/** Section 4: the one-line hints in empty step boxes. */
export const STEP_HINTS: Record<Letter, string> = {
  A: 'What happened, where and when? Give the measured gap against the standard, the impact and the target.',
  B: 'How was it narrowed down? Which causes were tested? State the confirmed root cause for occurrence and escape, with evidence.',
  C: 'Temporary containment and permanent solutions, each with owner, date and state. Tie each permanent solution to a root cause.',
  D: 'Result measured the same way as in A, over a stated period. Standards updated, by document name. Sharing with similar lines.',
};

export interface AppError {
  code: string;
  message: string;
}

export interface ConnectionProfile {
  host: string;
  port: number;
  database: string;
  sslMode: string;
  sslRootCert?: string | null;
}

export interface AppUser {
  id: string;
  roleName: string;
  displayName: string;
  email: string;
  isAdmin: boolean;
  active: boolean;
  defaultProjectId: string | null;
  lastProjectId: string | null;
}

export interface Project {
  id: string;
  name: string;
  code: string;
  description: string;
  ownerId: string;
  ownerName: string;
  aiEnabled: boolean;
  archivedAt: string | null;
  isMember: boolean;
  isOwner: boolean;
  memberCount: number;
}

export interface PickValue {
  id: string;
  listName: 'Category' | 'Source' | 'Area' | 'Product';
  value: string;
  projectId: string | null;
}

export interface Settings {
  stalledDays: number;
  linkBase: string;
}

export interface SheetRow {
  id: string;
  number: string;
  title: string;
  projectId: string;
  projectCode: string;
  ownerId: string;
  ownerName: string;
  stepStrip: string;
  status: string;
  currentStep: Letter | null;
  overdue: boolean;
  stalled: boolean;
  reopened: boolean;
  withdrawn: boolean;
  severity: string | null;
  category: string | null;
  area: string | null;
  registeredOn: string;
  targetCloseDate: string | null;
  closedAt: string | null;
  updatedAt: string;
  pendingReview: number;
}

export interface SheetFilter {
  mine?: boolean;
  quick?: string | null;
  projectId?: string | null;
  ownerId?: string | null;
  category?: string | null;
  area?: string | null;
  from?: string | null;
  to?: string | null;
  search?: string | null;
  sort?: string | null;
  descending?: boolean | null;
  page?: number | null;
  currentStep?: string | null;
  ageBand?: string | null;
  closedFrom?: string | null;
  reopenedFrom?: string | null;
  reopenReason?: string | null;
}

export interface SheetPage {
  rows: SheetRow[];
  total: number;
  page: number;
  pageSize: number;
}

export interface Sheet {
  id: string;
  number: string;
  title: string;
  projectId: string;
  projectName: string;
  projectCode: string;
  projectAiEnabled: boolean;
  ownerId: string;
  ownerName: string;
  teamMemberIds: string[];
  registeredOn: string;
  problemDate: string | null;
  targetCloseDate: string | null;
  category: string | null;
  severity: string | null;
  source: string | null;
  area: string | null;
  product: string | null;
  originWarningId: string | null;
  originTaskId: string | null;
  withdrawnAt: string | null;
  withdrawnReason: string | null;
  updatedAt: string;
  status: string;
  currentStep: Letter | null;
  closedAt: string | null;
  stepStrip: string;
  overdue: boolean;
  stalled: boolean;
  reopened: boolean;
  pendingReview: number;
  canEdit: boolean;
  isOwner: boolean;
}

export interface Step {
  id: string;
  sheetId: string;
  letter: Letter;
  content: unknown;
  plainText: string;
  state: 'open' | 'closed';
  closedAt: string | null;
  version: number;
  reopenCount: number;
  lockedBy: string | null;
  lockedByName: string | null;
  lockedAt: string | null;
  updatedAt: string;
}

export interface SheetDetail {
  sheet: Sheet;
  steps: Step[];
  link: string;
  openConflicts: number;
}

export interface SheetPatch {
  title: string;
  projectId: string;
  ownerId: string;
  teamMemberIds: string[];
  problemDate: string | null;
  targetCloseDate: string | null;
  category: string | null;
  severity: string | null;
  source: string | null;
  area: string | null;
  product: string | null;
}

export interface Snapshot {
  id: string;
  stepId: string;
  letter: Letter;
  content: unknown;
  plainText: string;
  version: number;
  authorName: string | null;
  reason: string;
  createdAt: string;
}

export interface AuditRow {
  id: number;
  actorName: string | null;
  objectType: string;
  action: string;
  reason: string | null;
  detail: Record<string, unknown>;
  at: string;
}

export interface Attachment {
  id: string;
  sheetId: string;
  fileName: string;
  version: number;
  blobHash: string;
  sizeBytes: number;
  fileType: string;
  readState: 'pending' | 'reading' | 'read' | 'unreadable' | 'stored_only' | 'needs_modern_format';
  readError: string | null;
  aiState: 'none' | 'pending' | 'running' | 'done' | 'failed';
  aiError: string | null;
  aiNote: string | null;
  summary: string | null;
  excludedSheets: string[];
  confirmsCount: number;
  addedBy: string;
  addedByName: string;
  addedAt: string;
  deletedAt: string | null;
  informsSteps: Letter[];
  sheetNames: string[];
  pendingImages: number;
}

export interface AddResult {
  added: Attachment[];
  duplicates: string[];
  errors: string[];
}

export interface Segment {
  id: string;
  attachmentId: string;
  seq: number;
  label: string;
  location: Record<string, unknown>;
  text: string;
  fromImage: boolean;
  needsImage: boolean;
  aiVisible: boolean;
  groupName: string | null;
}

export interface SourceView {
  segmentId: string;
  attachmentId: string;
  fileName: string;
  label: string;
  deleted: boolean;
}

export interface ImageData {
  mime: string;
  base64: string;
}

export interface Proposal {
  id: string;
  sheetId: string;
  attachmentId: string | null;
  letter: Letter | null;
  kind: 'add' | 'update' | 'conflict' | 'metadata';
  field: string | null;
  proposedText: string;
  currentText: string | null;
  sourceSegmentId: string | null;
  sourceLabel: string | null;
  quote: string | null;
  fromImage: boolean;
  why: string | null;
  state: string;
  stepClosed: boolean;
  createdAt: string;
}

export type ProposalDecision =
  | { action: 'accept'; text?: string | null }
  | { action: 'reject' }
  | { action: 'keepCurrent' }
  | { action: 'useNew' }
  | { action: 'keepBoth' };

export interface Finding {
  id: string;
  checkCode: string;
  letter: Letter;
  rank: number;
  text: string;
  why: string | null;
  sourceSegmentId: string | null;
  sourceLabel: string | null;
  quote: string | null;
  state: string;
  createdAt: string;
}

export interface ReviewOutcome {
  findings: Finding[];
  note: string | null;
}

export interface Suggestion {
  sentenceId: string;
  original: string;
  rewrite: string;
  why: string;
}

export interface CitedSource {
  segmentId: string | null;
  label: string;
  quote: string;
}

export interface AskAnswer {
  text: string;
  sources: CitedSource[];
  note: string | null;
  hasMore: boolean;
}

export interface AiMessage {
  id: string;
  role: 'user' | 'assistant' | 'summary';
  text: string;
  sources: CitedSource[];
  userName: string | null;
  createdAt: string;
}

export interface AiConfig {
  provider: string;
  baseUrl?: string | null;
  model: string;
  visionOk?: boolean;
}

export interface AiSettingsView {
  config: AiConfig | null;
  hasKey: boolean;
  ready: boolean;
}

export interface ConnectionTest {
  keyWorks: boolean;
  structuredOutputWorks: boolean;
  imageReadingWorks: boolean;
  message: string | null;
}

export interface WarningDraft {
  headline: string;
  whatHappened: string;
  whoShouldCheck: string[];
  quickCheck: string;
  severity: string;
  causeStatus: string;
}

export interface Warning {
  id: string;
  sheetId: string;
  sheetNumber: string;
  sheetTitle: string;
  headline: string;
  whatHappened: string;
  whoShouldCheck: string[];
  quickCheck: string;
  severity: string;
  causeStatus: string;
  authorId: string;
  authorName: string;
  postedAt: string;
  lastUpdateAt: string;
  withdrawnAt: string | null;
  withdrawnReason: string | null;
  archived: boolean;
  unread: boolean;
  updated: boolean;
  checksTotal: number;
  checksOpen: number;
  notAffected: number;
  affected: number;
  notApplicable: number;
  readCount: number;
}

export interface WarningUpdate {
  id: string;
  warningId: string;
  text: string;
  causeStatus: string;
  authorName: string;
  createdAt: string;
}

export interface WallFilter {
  unread?: boolean;
  severity?: string | null;
  area?: string | null;
  myTasks?: boolean;
  archive?: boolean;
  search?: string | null;
}

export interface Task {
  id: string;
  warningId: string;
  warningHeadline: string;
  description: string;
  assigneeId: string;
  assigneeName: string;
  targetArea: string;
  dueOn: string | null;
  result: string | null;
  closedAt: string | null;
  linkedA3Id: string | null;
  linkedA3Number: string | null;
}

export interface LabelCount {
  label: string;
  count: number;
}

export interface WarningReach {
  id: string;
  headline: string;
  readPercent: number;
  checksOpen: number;
  checksDone: number;
  affected: number;
}

export interface Management {
  byStep: LabelCount[];
  byAge: LabelCount[];
  overdue: SheetRow[];
  stalled: SheetRow[];
  closedCount: number;
  averageDaysToClose: number | null;
  reopenedCount: number;
  reopenedByReason: LabelCount[];
  warningReach: WarningReach[];
}

export interface MgmtFilter {
  projectId?: string | null;
  area?: string | null;
  from?: string | null;
  to?: string | null;
}

export interface MyWork {
  unreadWarnings: Warning[];
  unreadTotal: number;
  mySheets: SheetRow[];
  myTasks: Task[];
}

export interface EmailDraft {
  subject: string;
  text: string;
  html: string;
}

export type ExportLayout = 'quadrant' | 'accordion';
export type ExportFormat = 'pdf' | 'docx' | 'html';

export interface ExportOptions {
  layout: ExportLayout;
  includeAttachments: boolean;
  includeSourceNotes: boolean;
}

export interface JobEvent {
  attachmentId: string;
  sheetId: string;
  fileName: string;
  phase: 'running' | 'done';
  readState: string | null;
  added: number | null;
  confirms: number | null;
  note: string | null;
  error: string | null;
}

export type ResolvedLink = { kind: 'a3'; id: string } | { kind: 'warning'; id: string };
