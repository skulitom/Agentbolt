// `agentui <command>` — register the MCP server with the coding agents on this
// machine. The MCP server itself is `agentui-mcp`, launched by the agent.
import { apply, installSkill, plan, renderPlan, serverConfig } from "./install/install.js";
import { renderDiagnosis, runDoctor } from "./install/doctor.js";
import { allTargets } from "./install/targets.js";

const argv = process.argv.slice(2);
const command = argv[0] ?? "help";
const dryRun = argv.includes("--dry-run") || argv.includes("-n");
const only = argv
  .filter((a) => a.startsWith("--only="))
  .flatMap((a) => a.slice("--only=".length).split(","))
  .filter(Boolean);

const write = (text: string) => process.stdout.write(`${text}\n`);

function usage(): void {
  write(
    [
      "agentui — interactive UIs for coding agents",
      "",
      "Usage:",
      "  agentui install [--dry-run] [--only=id,id]   register with detected agents",
      "  agentui uninstall [--dry-run]                remove the registration",
      "  agentui doctor                               check it actually works, before you rely on it",
      "  agentui status                               show where it is registered",
      "",
      "Agent ids: " + allTargets().map((t) => t.id).join(", "),
      "",
      "Nothing is written for an agent that is not installed, existing entries are",
      "preserved, and every file is backed up before it is changed.",
    ].join("\n"),
  );
}

async function main(): Promise<void> {
  switch (command) {
    case "install":
    case "uninstall": {
      const entries = plan(command, only);
      const results = await apply(entries, dryRun);

      const config = serverConfig();
      write("");
      write(`${dryRun ? "Planned" : "Applied"} — ${config.command} ${config.args.join(" ")}`);
      write("");
      write(renderPlan(results, dryRun));
      write("");

      if (command === "install") {
        const skill = await installSkill(dryRun);
        write(
          skill.reason
            ? ` ! Skill              ${skill.reason}`
            : ` ${skill.written ? "+" : "~"} Skill              ${skill.written ? "installed" : "would install"} for Claude Code\n     ${skill.path}`,
        );
        write("");
      }

      const changed = results.filter((r) => r.written).length;
      const failed = results.filter((r) => r.error).length;
      const eligible = results.filter((r) => r.action !== "skip").length;

      if (!eligible) {
        write("No supported agents found on this machine — nothing to do.");
      } else if (dryRun) {
        write(`${eligible} config(s) would change. Re-run without --dry-run to apply.`);
      } else {
        write(`${changed} config(s) written${failed ? `, ${failed} failed` : ""}.`);
        if (changed) write("Restart any running agents so they pick up the new server.");
      }
      process.exitCode = failed ? 1 : 0;
      break;
    }

    case "doctor": {
      write("");
      write("Checking AgentUI the way your agents actually launch it…");
      write("");
      const results = await runDoctor();
      write(renderDiagnosis(results));
      write("");

      const failures = results.filter((r) => !r.ok);
      if (failures.length === 0) {
        write("All good. If an agent still cannot see the tools, restart it — MCP servers are");
        write("loaded at startup, so a fresh install is invisible to an already-running client.");
      } else {
        write(`${failures.length} problem(s) found.`);
      }
      process.exitCode = failures.length ? 1 : 0;
      break;
    }

    case "status": {
      const results = await apply(plan("install", only), true);
      write("");
      write(renderPlan(results, true));
      write("");
      break;
    }

    default:
      usage();
  }
}

main().catch((error: unknown) => {
  process.stderr.write(`agentui: ${error instanceof Error ? error.message : String(error)}\n`);
  process.exit(1);
});
