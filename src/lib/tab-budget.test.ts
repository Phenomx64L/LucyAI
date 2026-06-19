import { describe, it, expect } from 'vitest';
import { selectMessagesWithinBudget } from './tab-budget';

const msg = (id: number, tokens: number) => ({ id, tokens });

describe('selectMessagesWithinBudget', () => {
    it('returns null when already under budget', () => {
        const msgs = [msg(1, 10), msg(2, 10)];
        expect(selectMessagesWithinBudget(msgs, 100, 16)).toBeNull();
    });

    it('returns null for empty input', () => {
        expect(selectMessagesWithinBudget([], 100, 16)).toBeNull();
    });

    it('drops the oldest messages, keeps recent verbatim', () => {
        // 10 messages of 10 tokens = 100 total; budget 50; keepRecent 2.
        const msgs = Array.from({ length: 10 }, (_, i) => msg(i, 10));
        const r = selectMessagesWithinBudget(msgs, 50, 2)!;
        expect(r).not.toBeNull();
        expect(r.overflowedRecent).toBe(false);
        // recent 2 (20 tokens) + older kept until 50-20=30 budget → 3 older.
        expect(r.keptTokens).toBe(50);
        expect(r.kept.length).toBe(5);
        // recent block (ids 8,9) always last + in order
        expect(r.kept.slice(-2).map((m) => m.id)).toEqual([8, 9]);
        expect(r.droppedCount).toBe(5);
    });

    it('keeps only the recent block when it alone exceeds budget', () => {
        const msgs = Array.from({ length: 5 }, (_, i) => msg(i, 40)); // 200 total
        const r = selectMessagesWithinBudget(msgs, 50, 3)!; // recent 3 = 120 > 50
        expect(r.overflowedRecent).toBe(true);
        expect(r.kept.map((m) => m.id)).toEqual([2, 3, 4]);
        expect(r.droppedCount).toBe(2);
        expect(r.keptTokens).toBe(120);
    });

    it('treats missing tokens as 0', () => {
        const msgs = [{ id: 1 }, { id: 2 }, { id: 3, tokens: 0 }];
        expect(selectMessagesWithinBudget(msgs, 0, 1)).toBeNull(); // total 0 <= 0
    });
});
