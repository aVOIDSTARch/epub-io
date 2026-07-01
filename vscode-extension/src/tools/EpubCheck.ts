import * as vscode from "vscode";
import { execFile, execFileSync } from "child_process";
import * as fs from "fs";
import * as path from "path";

export interface CheckResult {
  code: number;
  stdout: string;
  stderr: string;
}

/**
 * Resolve a runnable `java` executable.
 *
 * This deliberately avoids the classic mistake behind the error
 * `spawn …/openjdk.jdk EACCES`: pointing Java at the `.jdk` *bundle directory*
 * instead of the `java` *binary*. We only ever return a path that is an actual
 * executable file (or the bare `java` name for PATH lookup).
 */
export function resolveJava(): string {
  const configured = vscode.workspace.getConfiguration("epubStudio").get<string>("javaPath");
  if (configured && configured.trim().length > 0) {
    return sanitizeJava(configured.trim());
  }

  // JAVA_HOME/bin/java
  const home = process.env.JAVA_HOME;
  if (home) {
    const candidate = path.join(home, "bin", "java");
    if (isExecutableFile(candidate)) {
      return candidate;
    }
  }

  // macOS: ask the system for a JavaHome, then append bin/java.
  if (process.platform === "darwin") {
    try {
      const jh = execFileSync("/usr/libexec/java_home", [], { encoding: "utf8" }).trim();
      const candidate = path.join(jh, "bin", "java");
      if (isExecutableFile(candidate)) {
        return candidate;
      }
    } catch {
      /* no JDK registered; fall through */
    }
    // Common Homebrew location, resolved to the binary (not the bundle dir).
    const brew = "/opt/homebrew/opt/openjdk/libexec/openjdk.jdk/Contents/Home/bin/java";
    if (isExecutableFile(brew)) {
      return brew;
    }
  }

  return "java";
}

/**
 * If a user (or a stale setting) points us at a `.jdk` bundle or a `JAVA_HOME`
 * directory, repair it to the real binary underneath rather than trying to
 * exec a directory.
 */
function sanitizeJava(p: string): string {
  try {
    const st = fs.statSync(p);
    if (st.isFile()) {
      return p;
    }
    if (st.isDirectory()) {
      const inside = [
        path.join(p, "bin", "java"),
        path.join(p, "Contents", "Home", "bin", "java"),
      ];
      for (const candidate of inside) {
        if (isExecutableFile(candidate)) {
          return candidate;
        }
      }
    }
  } catch {
    /* not a real path — let spawn surface a clear error, or PATH resolve it */
  }
  return p;
}

function isExecutableFile(p: string): boolean {
  try {
    const st = fs.statSync(p);
    if (!st.isFile()) {
      return false;
    }
    fs.accessSync(p, fs.constants.X_OK);
    return true;
  } catch {
    return false;
  }
}

/** Locate epubcheck.jar from settings; empty means "not configured". */
export function resolveEpubCheckJar(): string {
  return (vscode.workspace.getConfiguration("epubStudio").get<string>("epubcheckJar") ?? "").trim();
}

export async function validate(epubPath: string): Promise<CheckResult> {
  const jar = resolveEpubCheckJar();
  if (!jar) {
    throw new Error(
      "EPUBCheck jar not configured. Download epubcheck.jar from " +
        "https://github.com/w3c/epubcheck/releases and set 'epubStudio.epubcheckJar' to its path.",
    );
  }
  if (!fs.existsSync(jar)) {
    throw new Error(`Configured EPUBCheck jar does not exist: ${jar}`);
  }
  const java = resolveJava();

  return new Promise((resolve, reject) => {
    execFile(
      java,
      ["-jar", jar, epubPath],
      { maxBuffer: 32 * 1024 * 1024 },
      (err, stdout, stderr) => {
        const e = err as NodeJS.ErrnoException | null;
        if (e && e.code === "ENOENT") {
          reject(
            new Error(
              `Could not launch Java at '${java}'. Set 'epubStudio.javaPath' to your java ` +
                "binary (…/Contents/Home/bin/java), not a .jdk directory.",
            ),
          );
          return;
        }
        if (e && (e.code === "EACCES" || e.code === "EISDIR")) {
          reject(
            new Error(
              `'${java}' is not an executable java binary (got ${e.code}). It looks like a ` +
                "directory/bundle. Point 'epubStudio.javaPath' at …/Contents/Home/bin/java.",
            ),
          );
          return;
        }
        // EPUBCheck exits non-zero when it finds problems; that's a normal result.
        const code = e && typeof e.code === "number" ? Number(e.code) : e ? 1 : 0;
        resolve({ code, stdout: stdout ?? "", stderr: stderr ?? "" });
      },
    );
  });
}
