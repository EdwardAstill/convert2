import { execFile } from "node:child_process";
import { promisify } from "node:util";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { access } from "node:fs/promises";

const execFileAsync = promisify(execFile);

const __dirname = dirname(fileURLToPath(import.meta.url));
const PLUGIN_ROOT = resolve(__dirname, "..");
const PYTHON_DIR = resolve(PLUGIN_ROOT, "python");
const VENV_PYTHON = resolve(PYTHON_DIR, ".venv", "bin", "python");
const SCRIPT = resolve(PYTHON_DIR, "virtruvian_pdf.py");

let venvPromise: Promise<void> | null = null;

function ensureVenv(): Promise<void> {
  if (!venvPromise) {
    venvPromise = (async () => {
      try {
        await access(VENV_PYTHON);
      } catch {
        const setup = resolve(PYTHON_DIR, "setup.sh");
        await execFileAsync("bash", [setup]);
      }
    })();
  }
  return venvPromise;
}

export async function runPythonCommand(
  command: string,
  args: string[],
): Promise<unknown> {
  await ensureVenv();
  const { stdout, stderr } = await execFileAsync(
    VENV_PYTHON,
    [SCRIPT, command, ...args],
    { maxBuffer: 50 * 1024 * 1024, timeout: 120_000 },
  );
  if (stderr) {
    console.error(`Python stderr: ${stderr}`);
  }
  const result = JSON.parse(stdout);
  if (result && typeof result === "object" && "error" in result) {
    throw new Error(result.error);
  }
  return result;
}
