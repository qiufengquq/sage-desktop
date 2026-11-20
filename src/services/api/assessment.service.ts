import type {
  AnswerCheck,
  ApiResult,
  AssessmentModule,
  AssessmentModuleId,
  AssessmentOutcome,
  ExerciseAnswer,
  RequestOptions,
  SectionOutcome,
  TrainingTrack,
} from "@/types";
import { latency } from "@/services/mock/base";
import { assessmentModules } from "@/services/mock/assessment";
import { exerciseById } from "@/services/mock/exercises";
import { answerOf, scoreAnswer } from "@/lib/grading";
import { applyAssessmentSubmit, rebuildPlan } from "@/services/data/learning-db";
import { levelOfScore } from "@/lib/algorithms/ability";
import { commitDb, getDb } from "@/stores/learning-store";
import { COMMANDS } from "./contracts";
import { serve } from "./transport";

/** 最近一次成绩来自真实记录，不再是常量 */
function modulesWithHistory(): AssessmentModule[] {
  const db = getDb();
  return assessmentModules.map((module) => {
    const latest = db.assessmentAttempts
      .filter((attempt) => attempt.moduleId === module.id)
      .sort((a, b) => new Date(b.finishedAt).getTime() - new Date(a.finishedAt).getTime())[0];
    return {
      ...module,
      lastAttempt: latest ? { at: latest.finishedAt, score: latest.score } : null,
    };
  });
}

export function getAssessmentModules(
  options: RequestOptions = {},
): Promise<ApiResult<AssessmentModule[]>> {
  return serve(
    COMMANDS.assessmentModules,
    async () => {
      await latency(options.signal);
      return modulesWithHistory();
    },
    { request: options },
  );
}

const MODULE_TRACK: Record<AssessmentModuleId, TrainingTrack> = {
  vocabulary: "vocabulary",
  grammar: "grammar",
  speaking: "speaking",
};

/**
 * 测评中的单题即时校验。判分只在服务端（mock 模式用同一套本地规则）。
 */
export function checkAnswer(input: {
  moduleId: AssessmentModuleId;
  exerciseId: string;
  value: ExerciseAnswer["value"];
}): Promise<ApiResult<AnswerCheck>> {
  return serve(
    COMMANDS.assessmentCheck,
    async () => {
      const exercise = exerciseById(input.exerciseId);
      if (!exercise) {
        throw new Error(`题目不存在：${input.exerciseId}`);
      }
      const outcome = scoreAnswer(exercise, input.value);
      return {
        exerciseId: input.exerciseId,
        correct: outcome.correct,
        partialScore: outcome.partial,
        correctAnswer: answerOf(exercise),
        explanation: exercise.explanation,
      } satisfies AnswerCheck;
    },
    { args: { moduleId: input.moduleId, exerciseId: input.exerciseId, value: input.value } },
  );
}

/**
 * 提交测评：判分 → 保存记录 → 重算能力画像 → 按新画像重排计划。
 * 幂等键 `submissionId` **必填**（客户端生成的 UUID），与 Rust 侧契约一致。
 */
export function submitAssessment(input: {
  moduleId: AssessmentModuleId;
  answers: ExerciseAnswer[];
  /** 客户端生成的提交 id，用于重复提交时幂等 */
  submissionId: string;
}): Promise<ApiResult<AssessmentOutcome>> {
  return serve(
    COMMANDS.assessmentSubmit,
    async () => {
      await latency();
      const module = assessmentModules.find((m) => m.id === input.moduleId);
      if (!module) throw new Error(`not_found:${input.moduleId}`);

      const byId = new Map(input.answers.map((a) => [a.exerciseId, a]));
      const perSection: SectionOutcome[] = [];
      const exercises = module.sections.flatMap((section) => section.items);

      for (const section of module.sections) {
        const scored = section.items.map((item) => {
          const answer = byId.get(item.id);
          const outcome = answer
            ? scoreAnswer(item, answer.value)
            : { correct: false, partial: 0 };
          return { correct: outcome.correct, partial: outcome.partial };
        });
        const total = scored.length;
        const partialSum = scored.reduce((acc, s) => acc + s.partial, 0);
        perSection.push({
          sectionId: section.id,
          title: section.title,
          correct: scored.filter((s) => s.correct).length,
          total,
          score: total === 0 ? 0 : Math.round((partialSum / total) * 100),
        });
      }

      const scoredSections = perSection.filter((s) => s.total > 0);
      const score =
        scoredSections.length === 0
          ? 0
          : Math.round(
              scoredSections.reduce((acc, s) => acc + s.score, 0) / scoredSections.length,
            );
      const sorted = [...scoredSections].sort((a, b) => b.score - a.score);
      const strengths = sorted.slice(0, 2).map((s) => `${s.title}：${s.score} 分`);
      const weaknesses = sorted
        .slice(-2)
        .reverse()
        .filter((s) => s.score < 75)
        .map((s) => `${s.title}：${s.score} 分`);

      const weakest = sorted[sorted.length - 1];
      const weakestSection = module.sections.find((s) => s.id === weakest?.sectionId);
      const now = new Date();
      const finishedAt = now.toISOString();
      const durationMs = input.answers.reduce((acc, answer) => acc + answer.elapsedMs, 0);

      const outcome: AssessmentOutcome = {
        moduleId: module.id,
        score,
        accuracy: score,
        level: levelOfScore(score),
        perSection,
        strengths,
        weaknesses,
        recommendation: weakest
          ? `优先补强「${weakest.title}」。建议接下来 3 天每天 6 分钟定向训练，再复测一次看趋势。`
          : "各项水平接近，可以直接进入 B2 阶段的长句与学术表达训练。",
        finishedAt,
        followUp: weakestSection
          ? {
              label: `训练${weakest?.title ?? ""}`,
              kind: weakestSection.kind,
              track: MODULE_TRACK[module.id],
            }
          : null,
      };

      // mock 分支的幂等判定必须与后端同源：命中同一个 submissionId 即为重复提交
      const duplicate = getDb().assessmentAttempts.some(
        (attempt) => attempt.submissionId === input.submissionId,
      );
      outcome.duplicate = duplicate;

      commitDb((db) => {
        const applied = applyAssessmentSubmit(db, {
          attempt: {
            id: `aa_${now.getTime()}`,
            submissionId: input.submissionId,
            moduleId: module.id,
            score,
            accuracy: score,
            level: outcome.level,
            perSection: perSection.map((section) => ({
              sectionId: section.sectionId,
              title: section.title,
              correct: section.correct,
              total: section.total,
              score: section.score,
            })),
            answers: input.answers,
            strengths,
            weaknesses,
            recommendation: outcome.recommendation,
            minutes: Math.max(1, Math.round(durationMs / 60000)),
            finishedAt,
          },
          exercises,
          answers: input.answers,
          now,
        });
        if (applied.duplicate) return db;
        return rebuildPlan(applied.db, { reason: "assessment", now });
      });

      return outcome;
    },
    { args: input },
  );
}
