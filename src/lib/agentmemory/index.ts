// ── agentmemory — extended memory management modules ─────────────────────
//
// Cherry-picks from the agentmemory architecture:
//   • lessons   — dedicated lessons panel, separated from crystals
//   • sentinels — watchdog rules for proactive OS notifications
//   • patterns  — deterministic pattern detection on memory corpus
//   • verify    — cross-verification of contradictory memories

export { getLessons, promoteLesson, dismissLesson, getLessonProjects } from './lessons';
export type { Lesson, LessonFilter } from './lessons';

export { loadSentinels, saveSentinels, addSentinel, updateSentinel, deleteSentinel, toggleSentinel, checkSentinels, BUILTIN_TEMPLATES, opOptions } from './sentinels';
export type { SentinelRule, SentinelMetric, SentinelOp, SentinelCheckResult, SentinelTemplate } from './sentinels';

export { detectPatterns, patternKindLabel, patternKindIcon, confidenceColor } from './patterns';
export type { DetectedPattern, PatternReport, PatternKind } from './patterns';

export { verifyMemories, resolveContradiction, severityLabel, severityColor, resolutionLabel } from './verify';
export type { Contradiction, VerifyReport, VerifyMemory, Resolution, ConflictSeverity } from './verify';
