#!/usr/bin/env bun
/** Reconcile compatible main PRs onto stable and merge their verified batch. */
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { parseArgs } from "node:util";

export const BRANCH = "automation/stable-backports";
export const BREAKING_LABEL = "Breaking Changes";
const MARKER = "<!-- stable-backports:v1 -->";
const METADATA = "<!-- stable-backport-data:";
const SHA = /^[0-9a-f]{40}$/;
const VERSION = /^v?(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$/;

export interface SourceCommit {
  sha: string;
  number: number;
  title: string;
}

export interface Batch {
  pr: string;
  head: string;
  base: string;
  docs: boolean;
  conflicts: number[];
}

interface PullRequest {
  number: number;
  title: string;
  body: string | null;
  state: string;
  merged_at: string | null;
  merge_commit_sha: string | null;
  labels: { name: string }[];
  base: { ref: string };
  head: { ref: string; sha: string; repo: { full_name: string } };
  html_url: string;
}

export function version(value: string): number[] | null {
  const match = VERSION.exec(value);
  return match ? match.slice(1).map(Number) : null;
}

function compareVersions(a: number[], b: number[]): number {
  for (let i = 0; i < 3; i++) if (a[i] !== b[i]) return a[i] - b[i];
  return 0;
}

export function hasBreakingChanges(pr: Pick<PullRequest, "labels">): boolean {
  return pr.labels.some(label => label.name === BREAKING_LABEL);
}

export function needsDocs(paths: string[]): boolean {
  return paths.some(path => ["website/", "themes/", "crates/base/", "crates/story-web/"]
    .some(prefix => path.startsWith(prefix)) || ["Cargo.toml", "script/build-website-versions",
      "script/snapshot-website", "script/test-snapshot-website", ".github/workflows/test-docs.yml"].includes(path));
}

export function metadata(body: string): { base: string; commits: SourceCommit[] } {
  const line = body.split("\n").find(line => line.startsWith(METADATA) && line.endsWith(" -->"));
  if (!line || !body.includes(MARKER)) throw new Error("Missing automation metadata");
  const data = JSON.parse(line.slice(METADATA.length, -4));
  if (!SHA.test(data.base) || !Array.isArray(data.commits) || !data.commits.length
      || data.commits.some((item: SourceCommit) => !SHA.test(item.sha) || !Number.isSafeInteger(item.number))) {
    throw new Error("Invalid automation metadata");
  }
  return data;
}

export class StableBackport {
  constructor(readonly repo: string, readonly cwd = ".") {}

  spawn(args: string[], input?: string) {
    return Bun.spawnSync({ cmd: args, cwd: this.cwd,
      stdin: input === undefined ? "ignore" : Buffer.from(input), stdout: "pipe", stderr: "pipe" });
  }

  run(args: string[], input?: string): string {
    const result = this.spawn(args, input);
    if (!result.success) throw new Error(`${args[0]} failed: ${result.stderr.toString().trim()}`);
    return result.stdout.toString().trim();
  }

  git(...args: string[]): string {
    return this.run(["git", ...args]);
  }

  api<T = unknown>(endpoint: string, payload?: object, method = "GET"): T {
    const args = ["gh", "api", `repos/${this.repo}/${endpoint}`, "--method", method];
    if (payload !== undefined) args.push("--input", "-");
    return JSON.parse(this.run(args, payload === undefined ? undefined : JSON.stringify(payload))) as T;
  }

  ancestor(older: string, newer: string): boolean {
    const result = this.spawn(["git", "merge-base", "--is-ancestor", older, newer]);
    if (result.exitCode !== 0 && result.exitCode !== 1) throw new Error(result.stderr.toString());
    return result.exitCode === 0;
  }

  fetch() {
    this.git("fetch", "origin", "+refs/heads/main:refs/remotes/origin/main",
      "+refs/heads/stable:refs/remotes/origin/stable", "--tags");
  }

  crateVersion(ref: string): number[] | null {
    const manifest = Bun.TOML.parse(this.git("show", `${ref}:Cargo.toml`)) as {
      workspace: { dependencies: { "gpui-kit": { version: string } } };
    };
    return version(manifest.workspace.dependencies["gpui-kit"].version);
  }

  syncRelease() {
    // Reconcile on every run: concurrency can replace a pending release event.
    const runs = this.api<{ workflow_runs: { head_sha: string; head_branch: string; event: string }[] }>(
      "actions/workflows/release.yml/runs?status=success&per_page=100").workflow_runs;
    const published = new Set(runs.filter(run => run.event === "push")
      .map(run => `${run.head_branch}:${run.head_sha}`));
    const releases = this.git("tag", "--list", "v*").split("\n").flatMap(tag => {
      const number = version(tag);
      if (!number) return [];
      const sha = this.git("rev-parse", `${tag}^{commit}`);
      return published.has(`${tag}:${sha}`) ? [{ number, sha, tag }] : [];
    }).sort((a, b) => compareVersions(b.number, a.number));
    const release = releases[0];
    if (!release) return;
    const current = this.crateVersion("origin/stable");
    if (!current) throw new Error("stable must have a stable, non-prerelease crate version");
    if (compareVersions(release.number, current) <= 0) return;
    const released = this.crateVersion(release.sha);
    if (!released || compareVersions(released, release.number) !== 0) {
      throw new Error(`${release.tag} does not match the released crate version`);
    }
    // A patch tag may already be behind newly backported changes; keep them.
    if (this.ancestor(release.sha, "origin/stable")) return;
    this.api("git/refs/heads/stable", { sha: release.sha, force: true }, "PATCH");
    console.log(`stable now follows the successfully published ${release.tag}`);
    this.fetch();
  }

  openBatch(): PullRequest | undefined {
    const owner = this.repo.split("/")[0];
    return this.api<PullRequest[]>(`pulls?state=open&base=stable&head=${owner}:${BRANCH}&per_page=100`)
      .find(pr => pr.body?.includes(MARKER));
  }

  prepare(): Batch {
    this.fetch();
    this.syncRelease();
    const base = this.git("rev-parse", "origin/stable");
    const applied = new Set([...this.git("log", "origin/stable", "--format=%B")
      .matchAll(/^\(cherry picked from commit ([0-9a-f]{40})\)$/gm)].map(match => match[1]));
    const candidates: SourceCommit[] = [];
    for (const sha of this.git("rev-list", "--first-parent", "--reverse", "origin/stable..origin/main").split("\n").filter(Boolean)) {
      if (applied.has(sha)) continue;
      const pr = this.api<PullRequest[]>(`commits/${sha}/pulls`).find(pr => pr.merged_at
        && pr.base.ref === "main" && pr.merge_commit_sha === sha);
      if (!pr) console.log(`::warning::No merged main PR for ${sha}; direct commits need manual backporting`);
      else if (!hasBreakingChanges(pr)) candidates.push({ sha, number: pr.number, title: pr.title });
    }

    const existing = this.openBatch();
    const previous = this.git("ls-remote", "--heads", "origin", `refs/heads/${BRANCH}`);
    const expected = previous ? previous.split(/\s+/)[0] : "";
    this.git("config", "user.name", "github-actions[bot]");
    this.git("config", "user.email", "41898282+github-actions[bot]@users.noreply.github.com");
    this.git("switch", "--force-create", BRANCH, "origin/stable");
    const included: SourceCommit[] = [];
    const conflicts: number[] = [];
    for (const candidate of candidates) {
      if (this.git("rev-list", "--parents", "-n", "1", candidate.sha).split(/\s+/).length !== 2) {
        conflicts.push(candidate.number);
        console.log(`::warning::PR #${candidate.number} is not a squash merge; backport manually`);
        continue;
      }
      const result = this.spawn(["git", "cherry-pick", "-x", candidate.sha]);
      if (!result.success) {
        if (this.git("diff", "--name-only", "--diff-filter=U")) {
          conflicts.push(candidate.number);
          console.log(`::warning::PR #${candidate.number} conflicts with stable; backport manually`);
        } else if (this.git("diff", "--cached", "--name-only")
                   || !this.spawn(["git", "rev-parse", "--verify", "CHERRY_PICK_HEAD"]).success) {
          throw new Error(result.stderr.toString());
        }
        // Empty patches are already present; conflicts are retried on the next run.
        this.git("cherry-pick", "--abort");
      } else included.push(candidate);
    }

    const output: Batch = { pr: "", head: "", base, docs: false, conflicts };
    if (!included.length) {
      if (existing) this.api(`pulls/${existing.number}`, { state: "closed" }, "PATCH");
      return output;
    }
    const head = this.git("rev-parse", "HEAD");
    let body = `${MARKER}\n${METADATA}${JSON.stringify({ base, commits: included })} -->\n\n`
      + "Backport main PRs without the **Breaking Changes** label to the current stable series.\n\n"
      + included.map(item => `- #${item.number}: ${item.title}`).join("\n")
      + "\n\nThe Stable Branch workflow tests this exact head before merging.\n";
    if (conflicts.length) body += `\nConflicts requiring manual backporting: ${conflicts.map(number => `#${number}`).join(", ")}.\n`;
    this.git("push", `--force-with-lease=refs/heads/${BRANCH}:${expected}`, "origin", `HEAD:refs/heads/${BRANCH}`);
    const fields = { title: "release: Backport compatible main changes to stable", body };
    const pr = existing
      ? this.api<PullRequest>(`pulls/${existing.number}`, fields, "PATCH")
      : this.api<PullRequest>("pulls", { ...fields, head: BRANCH, base: "stable" }, "POST");
    this.api(`statuses/${head}`, { state: "pending", context: "Stable backport",
      description: "Checking the exact backport commit" }, "POST");
    Object.assign(output, { pr: String(pr.number), head,
      docs: needsDocs(this.git("diff", "--name-only", base, head).split("\n")) });
    console.log(pr.html_url);
    return output;
  }

  finish(number: number, head: string, base: string, result: string, url: string) {
    if (!SHA.test(head) || !SHA.test(base)) throw new Error("Invalid tested revision");
    const pr = this.api<PullRequest>(`pulls/${number}`);
    if (pr.state !== "open" || pr.head.sha !== head || pr.head.ref !== BRANCH
        || pr.base.ref !== "stable" || pr.head.repo.full_name !== this.repo) {
      throw new Error("Backport PR changed while CI was running");
    }
    const data = metadata(pr.body ?? "");
    if (data.base !== base) throw new Error("Backport base does not match the tested batch");
    const successful = result === "success";
    if (successful) {
      if (this.api<{ object: { sha: string } }>("git/ref/heads/stable").object.sha !== base) {
        throw new Error("stable advanced during CI; retry the workflow");
      }
      if (hasBreakingChanges(pr) || data.commits.some(item => hasBreakingChanges(this.api<PullRequest>(`pulls/${item.number}`)))) {
        throw new Error("Breaking Changes was added during CI; rebuild the batch");
      }
    }
    this.api(`statuses/${head}`, { state: successful ? "success" : "failure", context: "Stable backport",
      target_url: url, description: successful ? "Backport CI passed" : "Backport CI failed" }, "POST");
    if (successful) {
      // Preserve every original SHA through squash merging for idempotent reconciliation.
      const merged = this.api<{ merged: boolean; message: string }>(`pulls/${number}/merge`, {
        sha: head, merge_method: "squash",
        commit_title: `release: Backport compatible main changes to stable (#${number})`,
        commit_message: data.commits.map(item => `(cherry picked from commit ${item.sha})`).join("\n\n"),
      }, "PUT");
      if (!merged.merged) throw new Error(merged.message);
    }
  }
}

if (import.meta.main) {
  const { values, positionals } = parseArgs({ args: process.argv.slice(2), allowPositionals: true,
    options: { repo: { type: "string" }, pr: { type: "string" }, head: { type: "string" },
      base: { type: "string" }, result: { type: "string" }, url: { type: "string" } } });
  if (!values.repo || !/^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$/.test(values.repo)) throw new Error("Invalid repository");
  const automation = new StableBackport(values.repo);
  if (positionals[0] === "prepare") {
    writeFileSync(join(".git", "stable-backport-result.json"), JSON.stringify(automation.prepare()));
  } else if (positionals[0] === "finish" && values.pr && /^[1-9][0-9]*$/.test(values.pr)
             && values.head && values.base && ["success", "failure"].includes(values.result ?? "") && values.url) {
    automation.finish(Number(values.pr), values.head, values.base, values.result!, values.url);
  } else throw new Error("Use prepare --repo OWNER/REPO or finish --repo OWNER/REPO --pr NUMBER --head SHA --base SHA --result success|failure --url URL");
}
