// FaultDeck desktop UI — frontend.
// Talks to the Rust side exclusively via Tauri `invoke`; the Rust side owns
// the coordinator wire protocol. Mirrors docs/design/desktop-ui.md.

import { invoke } from "@tauri-apps/api/core";

type AgentIdentity = {
  id: string;
  capabilities: string[];
  last_seen_ms: number;
  online: boolean;
};

type FaultEvent = { timestamp_ms: number; kind: string; message: string };
type RestoreEvidence = { timestamp_ms: number; check: string; passed: boolean; detail: string };
type ExternalAssertion = { name: string; passed: boolean; detail: string };
type RunReport = {
  scenario_name: string;
  scenario_yaml: string;
  scenario_hash: string;
  injected_at_ms: number;
  rolled_back_at_ms: number | null;
  events: FaultEvent[];
  restore_evidence: RestoreEvidence[];
  external_assertions: ExternalAssertion[];
  replay_command: string;
};

type AgentResponse =
  | { status: "ok"; message: string }
  | { status: "error"; message: string }
  | { status: "registered"; id: string }
  | { status: "queued"; agent_id: string; pending: number }
  | { status: "agents"; agents: AgentIdentity[]; available: string[] }
  | { status: "report"; report: RunReport };

const $ = (id: string) => document.getElementById(id) as HTMLElement;
const input = (id: string) => $("host") ? (document.getElementById(id) as HTMLInputElement) : null;

let agents: AgentIdentity[] = [];
let currentAgentId: string | null = null;
let currentFault: AgentResponse["status"] extends never ? never : string = "";

function showBanner(msg: string) {
  const b = $("banner");
  b.textContent = msg;
  b.classList.remove("hidden");
}
function hideBanner() {
  $("banner").classList.add("hidden");
}

async function listAgents(): Promise<AgentResponse> {
  return invoke("coordinator_list", {
    host: ($("host") as HTMLInputElement).value,
    port: Number(($("port") as HTMLInputElement).value),
    token: ($("token") as HTMLInputElement).value || null,
  });
}

async function connect() {
  hideBanner();
  try {
    const resp = await listAgents();
    if (resp.status === "agents") {
      agents = resp.agents;
      renderFleet();
    } else {
      showBanner("Unexpected response: " + resp.status);
    }
  } catch (e) {
    showBanner(String(e));
  }
}

function renderFleet() {
  const box = $("agents");
  const empty = $("empty");
  box.innerHTML = "";
  ($("fleet-meta") as HTMLElement).textContent =
    agents.length === 0 ? "" : `${agents.filter((a) => a.online).length}/${agents.length} online`;
  empty.classList.toggle("hidden", agents.length > 0);

  for (const a of agents) {
    const card = document.createElement("div");
    card.className = "agent";

    const row = document.createElement("div");
    row.className = "row";
    const id = document.createElement("div");
    id.className = "id";
    const dot = document.createElement("span");
    dot.className = "status" + (a.online ? " online" : "");
    id.appendChild(dot);
    id.appendChild(document.createTextNode(a.id));
    row.appendChild(id);
    const applied = document.createElement("span");
    applied.className = "applied";
    applied.textContent = currentFault ? "fault applied" : "";
    row.appendChild(applied);
    card.appendChild(row);

    const caps = document.createElement("div");
    caps.className = "caps";
    for (const c of a.capabilities) {
      const chip = document.createElement("span");
      chip.className = "cap";
      chip.textContent = c;
      caps.appendChild(chip);
    }
    card.appendChild(caps);

    const ls = document.createElement("div");
    ls.className = "lastseen";
    ls.textContent = `last seen: ${new Date(a.last_seen_ms).toLocaleTimeString()}`;
    card.appendChild(ls);

    const actions = document.createElement("div");
    actions.className = "actions";
    const inj = document.createElement("button");
    inj.textContent = "Inject";
    inj.disabled = !a.online;
    inj.addEventListener("click", () => openInject(a.id));
    actions.appendChild(inj);
    const rb = document.createElement("button");
    rb.textContent = "Rollback";
    rb.disabled = !a.online || !currentFault;
    rb.addEventListener("click", () => rollback(a.id));
    actions.appendChild(rb);
    const rep = document.createElement("button");
    rep.textContent = "Report";
    rep.disabled = !currentFault;
    rep.addEventListener("click", () => loadReport(a.id));
    actions.appendChild(rep);
    card.appendChild(actions);

    box.appendChild(card);
  }
}

function openInject(agentId: string) {
  currentAgentId = agentId;
  ($("inject-target") as HTMLElement).textContent = `target: ${agentId}`;
  hideError();
  ($("inject-dialog") as HTMLDialogElement).showModal();
}

function hideError() {
  $("inject-error").classList.add("hidden");
}
function showError(msg: string) {
  const e = $("inject-error");
  e.textContent = msg;
  e.classList.remove("hidden");
}

function currentInjection() {
  const kind = ($("fault-kind") as HTMLSelectElement).value;
  const target: Record<string, unknown> = {};
  const src = ($("t-source") as HTMLInputElement).value.trim();
  const dst = ($("t-destination") as HTMLInputElement).value.trim();
  const port = ($("t-port") as HTMLInputElement).value.trim();
  if (src) target.source = src;
  if (dst) target.destination = dst;
  if (port) target.destination_port = Number(port);
  if (Object.keys(target).length === 0) {
    throw new Error("target requires a destination CIDR or port");
  }
  let fault: Record<string, unknown>;
  if (kind === "latency") {
    fault = { kind: "latency", milliseconds: Number(($("latency-ms") as HTMLInputElement).value) };
  } else if (kind === "loss") {
    fault = { kind: "loss", percent: Number(($("loss-percent") as HTMLInputElement).value) };
  } else {
    fault = { kind: "disconnect", all_traffic: ($("all-traffic") as HTMLInputElement).checked };
  }
  return { fault, target };
}

async function doInject() {
  if (!currentAgentId) return;
  hideError();
  let injection: unknown;
  try {
    injection = currentInjection();
  } catch (e) {
    showError(String((e as Error).message));
    return;
  }
  try {
    const resp = (await invoke("coordinator_inject", {
      host: ($("host") as HTMLInputElement).value,
      port: Number(($("port") as HTMLInputElement).value),
      token: ($("token") as HTMLInputElement).value || null,
      agentId: currentAgentId,
      injection,
    })) as AgentResponse;
    if (resp.status === "ok" || resp.status === "queued") {
      currentFault = (resp as { status: string }).status;
      ($("inject-dialog") as HTMLDialogElement).close();
      renderFleet();
    } else {
      showError((resp as { message: string }).message);
    }
  } catch (e) {
    showError(String(e));
  }
}

async function rollback(agentId: string) {
  hideBanner();
  try {
    const resp = (await invoke("coordinator_rollback", {
      host: ($("host") as HTMLInputElement).value,
      port: Number(($("port") as HTMLInputElement).value),
      token: ($("token") as HTMLInputElement).value || null,
      agentId,
    })) as AgentResponse;
    if (resp.status === "ok") {
      currentFault = "";
      renderFleet();
    } else {
      showBanner("Rollback failed: " + (resp as { message: string }).message);
    }
  } catch (e) {
    showBanner("Rollback error: " + String(e));
  }
}

async function loadReport(agentId: string) {
  hideBanner();
  const body = $("report-body");
  body.innerHTML = '<div class="empty">Loading report…</div>';
  try {
    // We only fetch the report when the agent reports its hash; the CLI
    // surface stores reports keyed by scenario_hash. Use the current fault
    // as the hash placeholder — the coordinator validates it.
    const resp = (await invoke("coordinator_report", {
      host: ($("host") as HTMLInputElement).value,
      port: Number(($("port") as HTMLInputElement).value),
      token: ($("token") as HTMLInputElement).value || null,
      agentId,
      scenarioHash: currentFault || "latest",
    })) as AgentResponse;
    if (resp.status === "report") {
      renderReport(resp.report);
    } else {
      body.innerHTML = `<div class="empty">No report: ${(resp as { message?: string }).message ?? resp.status}</div>`;
    }
  } catch (e) {
    body.innerHTML = `<div class="empty">Report error: ${String(e)}</div>`;
  }
}

function renderReport(r: RunReport) {
  const body = $("report-body");
  body.innerHTML = "";
  const h = document.createElement("h3");
  h.textContent = r.scenario_name || r.scenario_hash;
  body.appendChild(h);

  const meta = document.createElement("p");
  meta.className = "meta";
  meta.textContent = `injected ${new Date(r.injected_at_ms).toLocaleString()}` +
    (r.rolled_back_at_ms ? ` · rolled back ${new Date(r.rolled_back_at_ms).toLocaleString()}` : " · not rolled back");
  body.appendChild(meta);

  const yamlTitle = document.createElement("p");
  yamlTitle.textContent = "Scenario";
  body.appendChild(yamlTitle);
  const pre = document.createElement("pre");
  pre.textContent = r.scenario_yaml;
  body.appendChild(pre);

  const tlTitle = document.createElement("p");
  tlTitle.textContent = "Timeline";
  body.appendChild(tlTitle);
  const tl = document.createElement("ul");
  tl.className = "tl";
  for (const e of r.events) {
    const li = document.createElement("li");
    li.textContent = `[${new Date(e.timestamp_ms).toLocaleTimeString()}] ${e.kind}: ${e.message}`;
    tl.appendChild(li);
  }
  body.appendChild(tl);

  const evTitle = document.createElement("p");
  evTitle.textContent = "Restore evidence";
  body.appendChild(evTitle);
  const evT = document.createElement("table");
  const hdr = document.createElement("tr");
  for (const col of ["check", "passed", "detail"]) {
    const th = document.createElement("th");
    th.textContent = col;
    hdr.appendChild(th);
  }
  evT.appendChild(hdr);
  for (const ev of r.restore_evidence) {
    const tr = document.createElement("tr");
    const td1 = document.createElement("td");
    td1.textContent = ev.check;
    const td2 = document.createElement("td");
    td2.textContent = String(ev.passed);
    td2.className = ev.passed ? "pass" : "fail";
    const td3 = document.createElement("td");
    td3.textContent = ev.detail;
    tr.append(td1, td2, td3);
    evT.appendChild(tr);
  }
  body.appendChild(evT);

  const rep = document.createElement("p");
  rep.textContent = "Replay";
  body.appendChild(rep);
  const repCode = document.createElement("code");
  repCode.textContent = r.replay_command;
  body.appendChild(repCode);
}

function wireFaultKind() {
  const kind = ($("fault-kind") as HTMLSelectElement).value;
  ($("latency-field") as HTMLElement).classList.toggle("hidden", kind !== "latency");
  ($("loss-field") as HTMLElement).classList.toggle("hidden", kind !== "loss");
  ($("disconnect-field") as HTMLElement).classList.toggle("hidden", kind !== "disconnect");
}

function init() {
  ($("connect") as HTMLButtonElement).addEventListener("click", connect);
  ($("refresh") as HTMLButtonElement).addEventListener("click", connect);
  ($("fault-kind") as HTMLSelectElement).addEventListener("change", wireFaultKind);
  ($("inject-submit") as HTMLButtonElement).addEventListener("click", doInject);
  wireFaultKind();
  connect();
}

document.addEventListener("DOMContentLoaded", init);
