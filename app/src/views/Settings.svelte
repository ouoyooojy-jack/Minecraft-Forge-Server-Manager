<!--
  App settings.

  Only the theme so far, and it is real: it applies immediately and survives a
  restart. The .pen's other two fields (HTTP timeout, progress rate) are
  constants in the core with no settings module behind them yet — inputs that
  silently discard what the user types would be worse than not showing them.
-->
<script lang="ts">
  import { onMount } from "svelte";

  import Button from "../lib/Button.svelte";
  import {
    getAgentSettings,
    reportIssue,
    onCoreEvent,
    regenerateAgentToken,
    setAgentSettings,
  } from "../lib/api";
  import { errorMessage, type AgentSettings } from "../lib/types";

  /** How many recent connections to keep on screen. Enough to see what just
   *  happened, not a log file. */
  const ACTIVITY_LINES = 8;

  let agent = $state<AgentSettings | null>(null);
  let agentError = $state<string | null>(null);
  let activity = $state<string[]>([]);
  let busy = $state(false);
  let showToken = $state(false);

  async function apply(enabled: boolean, port: number) {
    busy = true;
    try {
      agent = await setAgentSettings(enabled, port);
      agentError = null;
    } catch (e) {
      // The switch goes back to what the core actually did — a port already in
      // use must not leave a toggle claiming to be on.
      agentError = errorMessage(e);
      agent = await getAgentSettings();
    } finally {
      busy = false;
    }
  }

  async function regenerate() {
    busy = true;
    try {
      agent = await regenerateAgentToken();
      agentError = null;
    } catch (e) {
      agentError = errorMessage(e);
    } finally {
      busy = false;
    }
  }

  onMount(() => {
    getAgentSettings()
      .then((s) => (agent = s))
      .catch((e) => (agentError = errorMessage(e)));

    const unlisten = onCoreEvent((event) => {
      if (event.type !== "agentActivity") return;
      const stamp = new Date().toLocaleTimeString("zh-TW", { hour12: false });
      activity = [`${stamp} ${event.line}`, ...activity].slice(0, ACTIVITY_LINES);
    });
    return () => void unlisten.then((fn) => fn());
  });

</script>

<section class="page">
  <header><h1>設定</h1></header>

  <div class="setting">
    <h2>回報與建議</h2>
    <p class="sub">
      問題和建議都走同一個入口，表單第一行才分。在 GitHub 上開一則 issue，
      版本和該回答的問題都會先填好。送出前你會看到全部內容，
      沒有任何東西是從這裡自動傳出去的。
    </p>
    <div class="feedback">
      <Button onclick={reportIssue}>開一則 issue</Button>
    </div>
    <p class="sub small">
      回報當機或啟動失敗時，先到「伺服器頁 → 設定 → 原始檔 → 匯出診斷檔」拿到 zip，
      再拖進 issue。裡面有主控台、Minecraft 的 log、當機報告和模組清單——
      也有玩家名稱，附不附上由你決定。
    </p>
  </div>

  <div class="setting">
    <h2>遠端存取</h2>
    <p class="sub">
      讓朋友從他的 Mc Server Manager 編輯這台電腦上的伺服器設定檔。只開放
      server.properties、user_jvm_args.txt、run.bat 三個檔案，只接受私有網路
      與 VPN 的連線。
    </p>

    {#if agent}
      <div class="row">
        <label class="toggle">
          <input
            type="checkbox"
            checked={agent.enabled}
            disabled={busy}
            onchange={(e) => apply(e.currentTarget.checked, agent!.port)}
          />
          <span>{agent.enabled ? "已開啟" : "已關閉"}</span>
        </label>

        <label class="port">
          <span>連接埠</span>
          <input
            type="number"
            value={agent.port}
            disabled={busy}
            onchange={(e) => apply(agent!.enabled, Number(e.currentTarget.value))}
          />
        </label>
      </div>

      <div class="row token-row">
        <span class="label">配對碼</span>
        <code class="token">{showToken || !agent.enabled ? agent.token : "••••-••••-••••-••••"}</code>
        <Button onclick={() => (showToken = !showToken)}>
          {showToken ? "隱藏" : "顯示"}
        </Button>
        <Button onclick={() => navigator.clipboard.writeText(agent!.token)}>複製</Button>
        <Button onclick={regenerate} disabled={busy}>換一組</Button>
      </div>

      <p class="note">
        把配對碼和你的 VPN 位址給對方就能連。換一組之後，舊的配對碼立刻失效——那是收回權限的唯一方法。
      </p>

      {#if agentError}
        <p class="agent-error" role="alert">{agentError}</p>
      {/if}

      {#if activity.length}
        <div class="activity">
          <h3>最近的連線</h3>
          <ul>
            {#each activity as line, i (line + i)}
              <li>{line}</li>
            {/each}
          </ul>
        </div>
      {/if}
    {/if}
  </div>

  <p class="todo">HTTP 逾時與進度更新頻率目前是核心裡的常數，還沒有設定模組。</p>
</section>

<style>
  .feedback {
    display: flex;
    gap: 10px;
    padding-top: 4px;
  }

  .sub.small {
    padding-top: 10px;
    color: var(--faint);
    font-size: var(--font-small);
  }

  .page {
    height: 100%;
    display: flex;
    flex-direction: column;
    gap: var(--gap-section);
    padding: 34px 40px 32px;
    overflow: auto;
  }

  /* A wordmark, not a heading. On a geometric face the authority comes from
     the drawing — even stroke, circular bowls — and a heavy weight destroys
     exactly that by thickening the monoline into a slab. Weight stays at 500
     and the letters are pulled together instead; that tightening is what makes
     a large geometric line read as set rather than merely enlarged. */
  h1 {
    margin: 0;
    font-family: var(--font-display);
    font-size: var(--font-hero);
    font-weight: 500;
    letter-spacing: -0.02em;
  }

  .row {
    display: flex;
    align-items: center;
    gap: 14px;
    margin-top: 14px;
  }

  .toggle {
    display: flex;
    align-items: center;
    gap: 10px;
    font-size: var(--font-body);
  }

  .toggle input {
    appearance: none;
    width: 39px;
    height: 22px;
    border-radius: var(--radius-pill);
    background: var(--wash-2);
    transition: background-color 140ms var(--ease);
  }

  .toggle input::after {
    content: "";
    display: block;
    width: 16px;
    height: 16px;
    margin: 3px;
    border-radius: 50%;
    background: var(--surface);
    transition: transform 140ms var(--ease);
  }

  .toggle input:checked {
    background: var(--accent);
  }

  .toggle input:checked::after {
    transform: translateX(17px);
  }

  .port {
    display: flex;
    align-items: center;
    gap: 8px;
    color: var(--muted);
    font-size: var(--font-small);
  }

  .port input,
  .token {
    height: 34px;
    padding: 0 12px;
    display: inline-flex;
    align-items: center;
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius-input);
    color: var(--fg);
    font-family: var(--font-mono);
    font-size: var(--font-small);
  }

  .port input {
    width: 96px;
    user-select: text;
  }

  .token-row .label {
    color: var(--muted);
    font-size: var(--font-small);
  }

  .token {
    letter-spacing: 0.12em;
    user-select: text;
  }

  .agent-error {
    margin-top: 12px;
    padding: 10px 12px;
    background: color-mix(in srgb, var(--error) 10%, transparent);
    border-radius: var(--radius-input);
    color: var(--error);
    font-size: var(--font-small);
  }

  .activity {
    margin-top: 16px;
    padding-top: 14px;
    border-top: 1px solid var(--border);
  }

  .activity h3 {
    color: var(--muted);
    font-size: var(--font-small);
    font-weight: 500;
  }

  .activity ul {
    margin-top: 8px;
    list-style: none;
    display: flex;
    flex-direction: column;
    gap: 5px;
    font-family: var(--font-mono);
    font-size: var(--font-tiny);
    color: var(--muted);
    user-select: text;
  }

  .setting {
    max-width: 720px;
    padding: 20px;
    border: 1px solid var(--border);
    border-radius: var(--radius-card);
    background: var(--surface-veil);
  }

  h2 {
    margin: 0;
    font-size: 15px;
    font-weight: 600;
  }

  .sub {
    margin: 4px 0 0;
    font-size: var(--font-small);
    color: var(--muted);
  }

  .row {
    display: flex;
    align-items: center;
    gap: 14px;
    margin-top: 14px;
  }

  .toggle {
    display: flex;
    align-items: center;
    gap: 10px;
    font-size: var(--font-body);
  }

  .toggle input {
    appearance: none;
    width: 39px;
    height: 22px;
    border-radius: var(--radius-pill);
    background: var(--wash-2);
    transition: background-color 140ms var(--ease);
  }

  .toggle input::after {
    content: "";
    display: block;
    width: 16px;
    height: 16px;
    margin: 3px;
    border-radius: 50%;
    background: var(--surface);
    transition: transform 140ms var(--ease);
  }

  .toggle input:checked {
    background: var(--accent);
  }

  .toggle input:checked::after {
    transform: translateX(17px);
  }

  .port {
    display: flex;
    align-items: center;
    gap: 8px;
    color: var(--muted);
    font-size: var(--font-small);
  }

  .port input,
  .token {
    height: 34px;
    padding: 0 12px;
    display: inline-flex;
    align-items: center;
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius-input);
    color: var(--fg);
    font-family: var(--font-mono);
    font-size: var(--font-small);
  }

  .port input {
    width: 96px;
    user-select: text;
  }

  .token-row .label {
    color: var(--muted);
    font-size: var(--font-small);
  }

  .token {
    letter-spacing: 0.12em;
    user-select: text;
  }

  .agent-error {
    margin-top: 12px;
    padding: 10px 12px;
    background: color-mix(in srgb, var(--error) 10%, transparent);
    border-radius: var(--radius-input);
    color: var(--error);
    font-size: var(--font-small);
  }

  .activity {
    margin-top: 16px;
    padding-top: 14px;
    border-top: 1px solid var(--border);
  }

  .activity h3 {
    color: var(--muted);
    font-size: var(--font-small);
    font-weight: 500;
  }

  .activity ul {
    margin-top: 8px;
    list-style: none;
    display: flex;
    flex-direction: column;
    gap: 5px;
    font-family: var(--font-mono);
    font-size: var(--font-tiny);
    color: var(--muted);
    user-select: text;
  }

  .setting {
    max-width: 720px;
    padding: 20px;
    border: 1px solid var(--border);
    border-radius: var(--radius-card);
    background: var(--surface-veil);
  }

  h2 {
    margin: 0;
    font-size: 15px;
    font-weight: 600;
  }

  .sub {
    margin: 4px 0 0;
    font-size: var(--font-small);
    color: var(--muted);
  }



  .note {
    margin: 16px 0 0;
    padding-top: 14px;
    border-top: 1px solid var(--border);
    font-size: var(--font-tiny);
    color: var(--faint);
  }

  .todo {
    margin: 0;
    font-size: var(--font-tiny);
    color: var(--muted);
  }
</style>
