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
  import Icon from "../lib/Icon.svelte";
  import {
    getAgentSettings,
    onCoreEvent,
    regenerateAgentToken,
    setAgentSettings,
  } from "../lib/api";
  import { errorMessage, type AgentSettings } from "../lib/types";
  import { THEMES, theme, type ThemeName } from "../lib/theme.svelte";

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

  /**
   * A swatch has to paint the palette it *offers*, not the one in use, so these
   * are the only literal colours in the app. Keep them in step with app.css.
   */
  const SWATCH: Record<ThemeName, Record<string, string>> = {
    dark: { bg: "#131316", rail: "#0e0e11", surface: "#1b1b20", border: "#2b2b32", accent: "#e8e8ec" },
    light: { bg: "#f7f8f9", rail: "#eef0f3", surface: "#ffffff", border: "#e3e6ea", accent: "#16181c" },
    claude: { bg: "#faf9f7", rail: "#f2efea", surface: "#ffffff", border: "#e6e1d9", accent: "#b4530a" },
  };

  const swatchVars = (name: ThemeName) =>
    Object.entries(SWATCH[name])
      .map(([k, v]) => `--s-${k}:${v}`)
      .join(";");
</script>

<section class="page">
  <header><h1>設定</h1></header>

  <div class="setting">
    <h2>主題</h2>
    <p class="sub">立即套用，並記住到下次啟動。</p>

    <div class="grid">
      {#each THEMES as option (option.name)}
        <button
          class="tile"
          class:active={theme.name === option.name}
          onclick={() => theme.set(option.name)}
          aria-pressed={theme.name === option.name}
        >
          <!-- A miniature of the window, so you can see a palette without
               having to switch to it first. -->
          <span class="swatch" style={swatchVars(option.name)}>
            <span class="s-rail"></span>
            <span class="s-body">
              <span class="s-card"></span>
              <span class="s-accent"></span>
            </span>
          </span>
          <span class="name">
            {option.label}
            {#if theme.name === option.name}
              <span class="check"><Icon name="check" size={14} strokeWidth={2.5} /></span>
            {/if}
          </span>
          <span class="desc">{option.desc}</span>
        </button>
      {/each}
    </div>

    <p class="note">
      標題列的按鈕只在深色與淺色之間切換，並會記得你在這裡選的淺色主題。
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
  .page {
    height: 100%;
    display: flex;
    flex-direction: column;
    gap: var(--gap-section);
    padding: 34px 40px 32px;
    overflow: auto;
  }

  h1 {
    margin: 0;
    font-size: var(--font-hero);
    font-weight: 600;
    letter-spacing: -0.4px;
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
    box-shadow: var(--shadow);
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
    box-shadow: var(--shadow), var(--edge-highlight);
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

  .grid {
    margin-top: 16px;
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(160px, 1fr));
    gap: var(--gap-field);
  }

  .tile {
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 10px;
    border: 1px solid var(--border);
    border-radius: var(--radius-button);
    background: transparent;
    text-align: left;
    transition: border-color 130ms var(--ease), background-color 130ms var(--ease);
  }
  .tile:hover {
    background: var(--wash);
  }
  .tile.active {
    border-color: var(--accent);
    background: color-mix(in srgb, var(--accent) 8%, transparent);
  }

  .swatch {
    height: 62px;
    display: flex;
    border: 1px solid var(--s-border);
    border-radius: 6px;
    background: var(--s-bg);
    overflow: hidden;
  }
  .s-rail {
    width: 14px;
    flex: none;
    background: var(--s-rail);
  }
  .s-body {
    flex: 1;
    padding: 8px;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .s-card {
    height: 20px;
    border-radius: 4px;
    background: var(--s-surface);
    border: 1px solid var(--s-border);
  }
  .s-accent {
    height: 10px;
    width: 46px;
    border-radius: var(--radius-pill);
    background: var(--s-accent);
  }

  .name {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 13px;
    font-weight: 600;
  }
  .check {
    display: flex;
    color: var(--accent);
  }
  .desc {
    font-size: var(--font-tiny);
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
