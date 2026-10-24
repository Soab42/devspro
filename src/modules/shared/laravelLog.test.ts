import { describe, expect, it } from "vitest";
import { durationToMs, failureWindow, formatMs, parseLaravelLog, shiftStamp, summarize } from "./laravelLog";

const QUEUE = `
   INFO  Processing jobs from the [db-jobs] queue.

  2026-10-22 13:17:09 App\\Jobs\\LabJob ........................................ RUNNING
  2026-10-22 13:17:13 App\\Jobs\\LabJob ........................................ 3s DONE
  2026-10-22 13:17:13 App\\Mail\\LabMail ....................................... RUNNING
  2026-10-22 13:17:13 App\\Mail\\LabMail ................................... 272.83ms DONE
  2026-10-22 13:17:14 App\\Jobs\\LabJob ........................................ RUNNING
  2026-10-22 13:17:14 App\\Jobs\\LabJob ................................... 61.10ms FAIL
  2026-10-22 13:18:02 App\\Jobs\\LabJob ........................................ RUNNING
`;

describe("parseLaravelLog", () => {
  it("pairs queue:work start and end lines into runs", () => {
    const { runs, otherLines } = parseLaravelLog(QUEUE);
    expect(runs.map((r) => [r.name, r.status, r.started, r.finished, r.duration])).toEqual([
      ["App\\Jobs\\LabJob", "done", "2026-10-22 13:17:09", "2026-10-22 13:17:13", "3s"],
      ["App\\Mail\\LabMail", "done", "2026-10-22 13:17:13", "2026-10-22 13:17:13", "272.83ms"],
      ["App\\Jobs\\LabJob", "failed", "2026-10-22 13:17:14", "2026-10-22 13:17:14", "61.10ms"],
      ["App\\Jobs\\LabJob", "running", "2026-10-22 13:18:02", undefined, undefined],
    ]);
    expect(runs[1].durationMs).toBeCloseTo(272.83);
    expect(otherLines).toBe(1);
  });

  it("marks an unfinished job that was followed by another as unknown", () => {
    const { runs } = parseLaravelLog(`
  2026-10-22 13:17:09 App\\Jobs\\A ...... RUNNING
--- manual run at 2026-10-22 13:20:00 ---
  2026-10-22 13:20:01 App\\Jobs\\B ...... RUNNING`);
    expect(runs.map((r) => r.status)).toEqual(["unknown", "running"]);
  });

  it("reads schedule:run events and their descriptions", () => {
    const { runs } = parseLaravelLog(`
  2026-10-22 05:49:51 Running ["artisan" lab:heartbeat --task=every-minute-heartbeat] .. 1s DONE
  ⇂ every-minute-heartbeat
  2026-10-22 05:50:00 Running [Callback] .................................. 12.5ms FAIL
   INFO  No scheduled commands are ready to run.`);
    expect(runs).toMatchObject([
      { kind: "command", name: "artisan lab:heartbeat --task=every-minute-heartbeat", status: "done", started: "2026-10-22 05:49:51", description: "every-minute-heartbeat", durationMs: 1000 },
      { kind: "command", name: "Callback", status: "failed", durationMs: 12.5 },
    ]);
  });

  it("reads the Laravel 8 Processing / Processed format", () => {
    const { runs } = parseLaravelLog(
      "[2026-10-22 13:17:09][41] Processing: App\\Jobs\\A\n[2026-10-22 13:17:10][41] Processed:  App\\Jobs\\A\n[2026-10-22 13:17:11][42] Failed:     App\\Jobs\\B",
    );
    expect(runs.map((r) => [r.name, r.status])).toEqual([
      ["App\\Jobs\\A", "done"],
      ["App\\Jobs\\B", "failed"],
    ]);
  });

  it("copes with no dot leader, ANSI colours and multi-unit durations", () => {
    const { runs } = parseLaravelLog(
      "\u001b[2m2026-10-22 13:17:09\u001b[0m App\\Jobs\\A \u001b[33mRUNNING\u001b[0m\n2026-10-22 13:18:14 App\\Jobs\\A 1m 5s DONE",
    );
    expect(runs).toMatchObject([{ name: "App\\Jobs\\A", status: "done", duration: "1m 5s", durationMs: 65_000 }]);
  });

  it("reads -v lines and pairs them by job id", () => {
    const { runs } = parseLaravelLog(`
  2026-10-22 17:28:52 App\\Jobs\\A 226 database probe .. RUNNING
  2026-10-22 17:28:52 App\\Jobs\\A 227 database probe .. RUNNING
  2026-10-22 17:28:53 App\\Jobs\\A 227 database probe .. 1s 22MB DONE
  2026-10-22 17:28:55 App\\Jobs\\A 226 database probe .. 62.71ms 4MB FAIL`);
    expect(runs).toMatchObject([
      { name: "App\\Jobs\\A", jobId: "226", connection: "database", queue: "probe", status: "failed", duration: "62.71ms", memory: "4MB" },
      { name: "App\\Jobs\\A", jobId: "227", status: "done", duration: "1s", memory: "22MB" },
    ]);
  });

  it("keeps what a run printed as its output, without DevsPro's headers", () => {
    const { runs } = parseLaravelLog(`
  2026-10-22 13:00:00 App\\Jobs\\A ...... RUNNING
   Symfony\\Component\\ErrorHandler\\Error\\FatalError
  Type of App\\Mail\\LabMail::$subject must not be defined
--- manual run at 2026-10-22 13:01:00 ---
  2026-10-22 13:01:01 App\\Jobs\\B ...... RUNNING
  2026-10-22 13:01:02 App\\Jobs\\B ...... 1s DONE`);
    expect(runs[0].output).toEqual([
      "   Symfony\\Component\\ErrorHandler\\Error\\FatalError",
      "  Type of App\\Mail\\LabMail::$subject must not be defined",
    ]);
    expect(runs[1].output).toEqual([]);
  });

  it("gives a run the same key whether or not it has finished", () => {
    const head = "  2026-10-22 13:00:00 App\\Jobs\\A ...... RUNNING";
    const before = parseLaravelLog(head).runs[0].key;
    const after = parseLaravelLog(`${head}\n  2026-10-22 13:00:03 App\\Jobs\\A ...... 3s DONE`).runs[0].key;
    expect(after).toBe(before);
    // Two runs started in the same second stay distinct.
    const twice = parseLaravelLog(`${head}\n  2026-10-22 13:00:00 App\\Jobs\\A ...... 1ms DONE\n${head}`).runs;
    expect(new Set(twice.map((r) => r.key)).size).toBe(2);
  });

  it("finds nothing in output that is not Laravel's", () => {
    expect(parseLaravelLog("listening on :3000\nGET / 200").runs).toEqual([]);
  });
});

describe("summaries", () => {
  it("counts outcomes and averages the timed runs", () => {
    const s = summarize(parseLaravelLog(QUEUE).runs);
    expect(s).toMatchObject({ total: 4, done: 2, failed: 1, running: 1 });
    expect(s.averageMs).toBeCloseTo((3000 + 272.83 + 61.1) / 3);
  });

  it("widens a run into the window its error was logged in", () => {
    expect(shiftStamp("2026-12-31 23:59:59", 2)).toBe("2027-01-01 00:00:01");
    const [run] = parseLaravelLog(
      "  2026-10-22 13:00:00 App\\Jobs\\A .. RUNNING\n  2026-10-22 13:00:03 App\\Jobs\\A .. 3s FAIL",
    ).runs;
    expect(failureWindow(run)).toEqual({
      from: "2026-10-22 13:00:00",
      to: "2026-10-22 13:00:04",
      likelyFrom: "2026-10-22 13:00:02",
    });
  });

  it("formats durations at a readable scale", () => {
    expect(formatMs(61.1)).toBe("61ms");
    expect(formatMs(3000)).toBe("3.00s");
    expect(formatMs(65_000)).toBe("1m 5s");
    expect(durationToMs("nope")).toBeUndefined();
  });
});
