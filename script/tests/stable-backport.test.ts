import { afterEach, beforeEach, expect, test } from "bun:test";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { BREAKING_LABEL, BRANCH, metadata, needsDocs, StableBackport, version, type Batch } from "../stable-backport";

// Real cherry-picks in local test repositories; GitHub operations are simulated.
class Fixture extends StableBackport {
  readonly root: string;
  readonly remote: string;
  readonly prs = new Map<string, any>();
  batch: any = null;
  readonly merges: any[] = [];
  readonly statuses: any[] = [];
  readonly published: { sha: string; tag: string }[] = [];

  constructor() {
    const root = mkdtempSync(resolve(".git/stable-tests-"));
    const checkout = join(root, "checkout");
    mkdirSync(checkout);
    super("example/project", checkout);
    this.root = root;
    this.remote = join(root, "remote.git");
    this.git("init", "--bare", this.remote);
    this.git("init", "-b", "main");
    this.git("config", "user.name", "Test");
    this.git("config", "user.email", "test@example.com");
    this.write("Cargo.toml", '[workspace.dependencies]\ngpui-kit = { version = "0.7.1" }\n');
    this.write("file", "original\n");
    this.git("add", ".");
    this.git("commit", "-m", "Initial release");
    this.git("tag", "v0.7.1");
    this.git("branch", "stable");
    this.git("remote", "add", "origin", this.remote);
    this.git("push", "origin", "main", "stable", "--tags");
  }

  write(name: string, value: string) {
    writeFileSync(join(this.cwd, name), value);
  }

  text(name: string) {
    return readFileSync(join(this.cwd, name), "utf8");
  }

  addPR(number: number, name: string, content: string, labels: string[] = []): string {
    this.git("switch", "main");
    this.write(name, content);
    this.git("add", name);
    this.git("commit", "-m", `PR #${number}`);
    const sha = this.git("rev-parse", "HEAD");
    this.prs.set(sha, { number, title: `Change ${number}`, merged_at: "now", merge_commit_sha: sha,
      base: { ref: "main" }, labels: labels.map(name => ({ name })) });
    this.git("push", "origin", "main");
    return sha;
  }

  override api<T = unknown>(endpoint: string, payload?: object, method = "GET"): T {
    const fields = payload as any;
    let response: unknown;
    if (endpoint.startsWith("actions/workflows/")) {
      response = { workflow_runs: this.published.map(run => ({ head_sha: run.sha, head_branch: run.tag, event: "push" })) };
    } else if (endpoint.startsWith("commits/")) {
      const pr = this.prs.get(endpoint.split("/")[1]);
      response = pr ? [pr] : [];
    } else if (endpoint.startsWith("pulls?")) {
      response = this.batch?.state === "open" ? [this.batch] : [];
    } else if (endpoint === "pulls" && method === "POST") {
      this.batch = { ...fields, number: 9000, html_url: "https://example.test/pr/9000", state: "open", labels: [],
        head: { sha: this.git("rev-parse", "HEAD"), ref: BRANCH, repo: { full_name: this.repo } }, base: { ref: "stable" } };
      response = this.batch;
    } else if (endpoint === "pulls/9000") {
      if (method === "PATCH") {
        Object.assign(this.batch, fields);
        this.batch.head.sha = this.git("rev-parse", "HEAD");
      }
      response = this.batch;
    } else if (endpoint.endsWith("/merge")) {
      this.merges.push(fields);
      const sha = this.git("commit-tree", `${fields.sha}^{tree}`, "-p", this.git("rev-parse", "origin/stable"),
        "-m", fields.commit_message);
      this.git("push", "origin", `${sha}:refs/heads/stable`);
      this.batch.state = "closed";
      response = { merged: true };
    } else if (endpoint.startsWith("pulls/")) {
      response = [...this.prs.values()].find(pr => pr.number === Number(endpoint.split("/")[1]));
    } else if (endpoint.startsWith("statuses/")) {
      this.statuses.push(fields);
      response = fields;
    } else if (endpoint === "git/ref/heads/stable") {
      response = { object: { sha: this.git("ls-remote", "origin", "refs/heads/stable").split(/\s+/)[0] } };
    } else if (endpoint === "git/refs/heads/stable") {
      this.git("push", "--force", "origin", `${fields.sha}:refs/heads/stable`);
      response = fields;
    } else throw new Error(`Unexpected API call: ${method} ${endpoint}`);
    return response as T;
  }

  complete(batch: Batch, result = "success") {
    this.finish(Number(batch.pr), batch.head, batch.base, result, "https://example.test/run");
  }
}

let fixture: Fixture;
beforeEach(() => { fixture = new Fixture(); });
afterEach(() => { rmSync(fixture.root, { recursive: true, force: true }); });

test("excludes Breaking Changes and preserves idempotency after squash merging", () => {
  const skipped = fixture.addPR(1, "breaking", "new API", [BREAKING_LABEL]);
  const included = fixture.addPR(2, "fix", "compatible fix");
  const batch = fixture.prepare();
  expect(fixture.git("ls-files", "breaking")).toBe("");
  expect(fixture.text("fix")).toBe("compatible fix");
  expect(metadata(fixture.batch.body).commits.map(item => item.sha)).toEqual([included]);
  fixture.complete(batch);
  expect(fixture.merges[0].commit_message).not.toContain(skipped);
  expect(fixture.merges[0].commit_message).toContain(included);
  expect(fixture.prepare().pr).toBe("");
  expect(fixture.merges).toHaveLength(1);
});

test("conflicts do not block an independent compatible fix", () => {
  fixture.git("switch", "stable");
  fixture.write("file", "stable edit\n");
  fixture.git("commit", "-am", "Stable-only edit");
  fixture.git("push", "origin", "stable");
  fixture.addPR(1, "file", "main edit\n");
  fixture.addPR(2, "fix", "independent fix");
  const batch = fixture.prepare();
  expect(batch.conflicts).toEqual([1]);
  expect(fixture.text("file")).toBe("stable edit\n");
  expect(fixture.text("fix")).toBe("independent fix");
  expect(metadata(fixture.batch.body).commits.map(item => item.number)).toEqual([2]);
});

test("failed CI and labels added during verification prevent merging", () => {
  const sha = fixture.addPR(1, "fix", "fix");
  const batch = fixture.prepare();
  fixture.complete(batch, "failure");
  expect(fixture.statuses.at(-1).state).toBe("failure");
  expect(fixture.merges).toHaveLength(0);
  fixture.prs.get(sha).labels.push({ name: BREAKING_LABEL });
  expect(() => fixture.complete(batch)).toThrow("Breaking Changes was added");
  expect(fixture.prepare().pr).toBe("");
  expect(fixture.batch.state).toBe("closed");
});

test("changes to stable or the tested PR head prevent merging", () => {
  fixture.addPR(1, "fix", "fix");
  const batch = fixture.prepare();
  fixture.git("switch", "stable");
  fixture.write("other", "new stable fix");
  fixture.git("add", "other");
  fixture.git("commit", "-m", "Advance stable");
  fixture.git("push", "origin", "stable");
  expect(() => fixture.complete(batch)).toThrow("stable advanced");
  fixture.batch.head.sha = "f".repeat(40);
  expect(() => fixture.complete(batch)).toThrow("PR changed");
  expect(fixture.merges).toHaveLength(0);
});

test("only successful new stable releases promote stable; retries preserve later fixes", () => {
  const release = fixture.addPR(1, "Cargo.toml", '[workspace.dependencies]\ngpui-kit = { version = "0.8.0" }\n', [BREAKING_LABEL]);
  fixture.git("tag", "v0.8.0");
  fixture.git("push", "origin", "--tags");
  fixture.fetch();
  fixture.syncRelease();
  expect(fixture.git("rev-parse", "origin/stable")).not.toBe(release);
  // A successful prerelease at the same SHA does not prove the stable tag published.
  fixture.published.push({ sha: release, tag: "v0.8.0-alpha.1" });
  fixture.syncRelease();
  expect(fixture.git("rev-parse", "origin/stable")).not.toBe(release);
  fixture.published.push({ sha: release, tag: "v0.8.0" });
  fixture.syncRelease();
  expect(fixture.git("rev-parse", "origin/stable")).toBe(release);
  fixture.git("switch", "stable");
  fixture.git("reset", "--hard", "origin/stable");
  fixture.write("fix", "after release");
  fixture.git("add", "fix");
  fixture.git("commit", "-m", "Later stable fix");
  fixture.git("push", "origin", "stable");
  fixture.fetch();
  const later = fixture.git("rev-parse", "origin/stable");
  fixture.syncRelease();
  expect(fixture.git("rev-parse", "origin/stable")).toBe(later);
});

test("an already applied manual patch is empty and does not create a backport PR", () => {
  const sha = fixture.addPR(1, "fix", "fix");
  fixture.git("switch", "stable");
  fixture.git("cherry-pick", sha);
  fixture.git("push", "origin", "stable");
  const batch = fixture.prepare();
  expect(batch.pr).toBe("");
  expect(batch.conflicts).toEqual([]);
  expect(fixture.batch).toBeNull();
});

test("older releases do not move stable backwards", () => {
  fixture.published.push({ sha: fixture.git("rev-parse", "v0.7.1"), tag: "v0.7.1" });
  fixture.git("switch", "stable");
  fixture.write("Cargo.toml", '[workspace.dependencies]\ngpui-kit = { version = "0.7.2" }\n');
  fixture.git("commit", "-am", "Next stable patch");
  fixture.git("push", "origin", "stable");
  fixture.fetch();
  const stable = fixture.git("rev-parse", "origin/stable");
  fixture.syncRelease();
  expect(fixture.git("rev-parse", "origin/stable")).toBe(stable);
});

test("rejects prereleases and mirrors the documentation CI path filters", () => {
  expect(version("v0.8.0-alpha.1")).toBeNull();
  expect(version("v00.8.0")).toBeNull();
  expect(version("v0.8.0")).toEqual([0, 8, 0]);
  expect(needsDocs(["crates/base/src/input.rs"])).toBe(true);
  expect(needsDocs(["crates/component/src/button.rs"])).toBe(false);
});
