/**
 * AgentPanel 输出缓冲区属性测试 (fast-check)
 *
 * Covers:
 * - Property 15: Agent Panel 输出缓冲区限制 — 验证超过 5000 行时仅保留最近 5000 行
 *
 * **Validates: Requirements 13.3**
 */

import { describe, it, expect } from 'vitest';
import fc from 'fast-check';

// ─── Pure function extracted from AgentPanel component ──────────────────────
// Mirrors the buffer trimming logic in AgentPanel.tsx:
//   const next = [...prev, payload.content ?? ''];
//   if (next.length > MAX_OUTPUT_LINES) {
//     return next.slice(next.length - MAX_OUTPUT_LINES);
//   }
//   return next;

const MAX_OUTPUT_LINES = 5000;

/**
 * Appends a new line to the output buffer and trims to MAX_OUTPUT_LINES.
 * Returns the new buffer state.
 */
function appendToBuffer(prev: string[], newLine: string): string[] {
  const next = [...prev, newLine];
  if (next.length > MAX_OUTPUT_LINES) {
    return next.slice(next.length - MAX_OUTPUT_LINES);
  }
  return next;
}

/**
 * Processes a sequence of output lines through the buffer logic,
 * simulating the AgentPanel receiving lines one by one.
 */
function processLines(lines: string[]): string[] {
  let buffer: string[] = [];
  for (const line of lines) {
    buffer = appendToBuffer(buffer, line);
  }
  return buffer;
}

// ─── Arbitraries ────────────────────────────────────────────────────────────

/** Generate a random output line (0-100 chars, any printable + ANSI escape) */
const arbOutputLine = fc.string({ minLength: 0, maxLength: 100 });

/** Generate a sequence of output lines (varying length 0-10000) */
const arbOutputSequence = fc.array(arbOutputLine, { minLength: 0, maxLength: 10000 });

// ─── Property 15: Agent Panel 输出缓冲区限制 ────────────────────────────────
// **Validates: Requirements 13.3**

describe('Property 15: Agent Panel 输出缓冲区限制', () => {
  it('buffer never exceeds 5000 lines after processing any sequence of outputs', () => {
    fc.assert(
      fc.property(arbOutputSequence, (lines) => {
        const buffer = processLines(lines);
        expect(buffer.length).toBeLessThanOrEqual(MAX_OUTPUT_LINES);
      }),
      { numRuns: 100 },
    );
  });

  it('retained lines are always the most recent ones (last N lines of input)', () => {
    fc.assert(
      fc.property(arbOutputSequence, (lines) => {
        const buffer = processLines(lines);

        if (lines.length === 0) {
          expect(buffer).toEqual([]);
          return;
        }

        // The buffer should contain the last min(lines.length, MAX_OUTPUT_LINES) lines
        const expectedCount = Math.min(lines.length, MAX_OUTPUT_LINES);
        expect(buffer.length).toBe(expectedCount);

        // The retained lines should be the tail of the input sequence
        const expectedLines = lines.slice(lines.length - expectedCount);
        expect(buffer).toEqual(expectedLines);
      }),
      { numRuns: 100 },
    );
  });

  it('intermediate buffer states never exceed MAX_OUTPUT_LINES during incremental appends', () => {
    fc.assert(
      fc.property(arbOutputSequence, (lines) => {
        let buffer: string[] = [];
        for (const line of lines) {
          buffer = appendToBuffer(buffer, line);
          // At every step, the buffer must respect the limit
          expect(buffer.length).toBeLessThanOrEqual(MAX_OUTPUT_LINES);
        }
      }),
      { numRuns: 100 },
    );
  });

  it('buffer preserves order of most recent lines', () => {
    fc.assert(
      fc.property(
        fc.array(arbOutputLine, { minLength: 1, maxLength: 10000 }),
        (lines) => {
          const buffer = processLines(lines);
          // Verify that the relative order of lines in the buffer matches
          // the relative order in the original input
          for (let i = 1; i < buffer.length; i++) {
            // Since buffer is a contiguous tail slice, each element's position
            // in the original array should be monotonically increasing
            const startIdx = lines.length - buffer.length;
            expect(buffer[i - 1]).toBe(lines[startIdx + i - 1]);
            expect(buffer[i]).toBe(lines[startIdx + i]);
          }
        },
      ),
      { numRuns: 100 },
    );
  });
});
