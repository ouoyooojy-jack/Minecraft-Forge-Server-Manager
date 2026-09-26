<!--
  App settings: feedback, the playit.gg account, and remote access.

  Everything here applies the moment it is changed — there is no save button,
  so anything that cannot be taken back asks first.
-->
<script lang="ts">
  import { onMount } from "svelte";

  import Button from "../lib/Button.svelte";
  import Modal from "../lib/Modal.svelte";
  import {
    getAgentSettings,
    reportIssue,
    onCoreEvent,
    playitCancelClaim,
    playitClaimUrl,
    playitDeleteTunnel,
    playitSetAuto,
    playitFinishClaim,
    playitInstall,
    playitStart,
    playitStatus,
    playitStop,
    playitUnlink,
    regenerateAgentToken,
    setAgentSettings,
  } from "../lib/api";
  import { errorMessage, type AgentSettings, type PlayitStatus } from "../lib/types";

  /** How many recent connections to keep on screen. Enough to see what just
   *  happened, not a log file. */
  const ACTIVITY_LINES = 8;

  let agent = $state<AgentSettings | null>(null);
  let agentError = $state<string | null>(null);
  let activity = $state<string[]>([]);
  let busy = $state(false);
  let showToken = $state(false);
  /** The port as typed, applied only once it is a real port. */
  let portText = $state("");
  const portBad = $derived(!/^\d+$/.test(portText) || +portText < 1024 || +portText > 65535);

  /**
   * Which irreversible button is asking "are you sure": `unlink`, `regen`, or
   * `tunnel:<id>`. One at a time — a second one replaces the first.
   */
  let confirming = $state<string | null>(null);

  /**
   * The report being written.
   *
   * Composed here rather than on GitHub's page, because the page is where
   * people arrive already annoyed and leave without typing. A box in the app,
   * at the moment the thing went wrong, gets an actual sentence.
   */
  let reportOpen = $state(false);
  let reportTitle = $state("");
  let reportBody = $state("");
  let reportError = $state<string | null>(null);

  /** Matches `REPORT_MAX_CHARS` in the core, which is where it is enforced —
   *  this only makes the count visible while typing. */
  const REPORT_MAX = 1200;
  const reportLength = $derived(reportTitle.trim().length + reportBody.trim().length);
  const reportReady = $derived(
    reportTitle.trim() !== "" && reportBody.trim() !== "" && reportLength <= REPORT_MAX,
  );

  async function sendReport() {
    try {
      await reportIssue(reportTitle, reportBody);
      reportOpen = false;
      reportTitle = "";
      reportBody = "";
      reportError = null;
    } catch (e) {
      reportError = errorMessage(e);
    }
  }

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
      portText = String(agent?.port ?? portText);
    }
  }

  function applyPort() {
    if (!agent || portBad || +portText === agent.port) return;
    apply(agent.enabled, +portText);
  }

  async function regenerate() {
    confirming = null;
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

  /**
   * playit.gg state.
   *
   * `null` until the first status call comes back. Every button below refreshes
   * it afterwards rather than guessing the new state, because the agent and the
   * tunnel list live outside this app — playit's site can change either.
   */
  let playit = $state<PlayitStatus | null>(null);
  let playitError = $state<string | null>(null);
  let playitBusy = $state<string | null>(null);
  /** Shown while the browser is open, so the page is findable if it did not
   *  come to the front by itself. */
  let claimUrl = $state<string | null>(null);
  /** Which step of the one-button setup is in flight, for the button's label.
   *  Three commands behind one press is three different waits. */
  let playitStage = $state("");

  /** Addresses playit has not finished allocating yet. Tunnel creation returns
   *  before the hostname exists, so a tunnel appears here first. */
  const playitWaiting = $derived(playit?.pending ?? []);

  async function refreshPlayit() {
    try {
      playit = await playitStatus();
    } catch (e) {
      playitError = errorMessage(e);
    }
  }

  /** Run one playit step with the section's buttons disabled and the failure
   *  shown in one place. `label` is what is in flight, for the button text. */
  async function playitStep(label: string, step: () => Promise<void>) {
    playitBusy = label;
    playitError = null;
    try {
      await step();
    } catch (e) {
      playitError = errorMessage(e);
    } finally {
      playitBusy = null;
      playitStage = "";
      claimUrl = null;
      await refreshPlayit();
    }
  }

  /**
   * One button: download the agent, link the account, start the tunnel, and
   * switch on "public address follows the server".
   *
   * Every step is skipped when it is already done, so pressing it again after a
   * failure resumes instead of repeating. The only part that cannot be
   * automated is the sign-in, which happens on playit's own site — this app
   * must not hold anybody's playit account.
   */
  function setUpPlayit() {
    return playitStep("設定中", async () => {
      if (!playit?.installed) {
        playitStage = "下載代理程式…";
        await playitInstall();
      }
      if (!playit?.linked) {
        playitStage = "等你在瀏覽器裡按同意…";
        const [code, url] = await playitClaimUrl();
        claimUrl = url;
        await playitFinishClaim(code);
      }
      playitStage = "啟動隧道…";
      await playitStart();
      await playitSetAuto(true);
    });
  }

  onMount(() => {
    refreshPlayit();
    // Tunnel addresses are allocated on playit's side after the request
    // returns, so the list has to be re-read rather than waited on. Only while
    // linked: with no key there is nothing to ask about.
    const timer = setInterval(() => {
      if (playit?.linked && !playitBusy) refreshPlayit();
    }, 10_000);

    getAgentSettings()
      .then((s) => ((agent = s), (portText = String(s.port))))
      .catch((e) => (agentError = errorMessage(e)));

    const unlisten = onCoreEvent((event) => {
      if (event.type !== "agentActivity") return;
      const stamp = new Date().toLocaleTimeString("zh-TW", { hour12: false });
      activity = [`${stamp} ${event.line}`, ...activity].slice(0, ACTIVITY_LINES);
    });
    return () => {
      clearInterval(timer);
      void unlisten.then((fn) => fn());
    };
  });

</script>

<section class="page">
  <header><h1>設定</h1></header>

  <div class="setting">
    <h2>回報與建議</h2>
    <p class="sub">
      在這裡寫，按下送出會開啟 GitHub 並把你寫的內容填好，程式版本也會附在後面。
      送出前你會看到全部內容，沒有任何東西是從這裡自動傳出去的。
    </p>
    <div class="feedback">
      <Button onclick={() => (reportOpen = true)}>寫一則回報</Button>
    </div>
    <p class="sub small">
      回報當機或啟動失敗時，先到「伺服器頁 → 伺服器設定 → 原始檔 → 匯出診斷檔」拿到 zip，
      再拖進 issue。裡面有主控台、Minecraft 的 log、當機報告和模組清單——
      也有玩家名稱，附不附上由你決定。
    </p>
  </div>

  <div class="setting">
    <h2>公開連線（playit.gg）</h2>
    <p class="sub">
      不用設定路由器的通訊埠轉發，也不用管中華電信給你的是不是真實 IP：由這台電腦主動連出去，
      朋友只要輸入一個網址就能進來。用的是 playit.gg 的免費隧道服務，連線會經過他們的伺服器。
    </p>

    {#if playit}
      {#if !playit.installed || !playit.linked}
        <div class="row">
          <Button variant="primary" disabled={playitBusy !== null} onclick={setUpPlayit}>
            {playitStage || "開啟公開連線"}
          </Button>
          <span class="hint">
            按一次就好：下載 playit 的代理程式、開瀏覽器讓你在 playit.gg 註冊或登入一次、
            啟動隧道，之後每次開伺服器都會自動有公開位址。密碼不會經過這個程式。
          </span>
        </div>
        {#if claimUrl}
          <div class="row token-row">
            <span class="label">授權網址</span>
            <code class="token addr">{claimUrl}</code>
            <Button onclick={() => navigator.clipboard.writeText(claimUrl!)}>複製</Button>
            <Button onclick={playitCancelClaim}>取消</Button>
          </div>
          <p class="note">瀏覽器沒跳出來的話，複製這個網址自己貼上。不想連了就按取消。</p>
        {/if}
      {:else}
        <div class="row">
          <label class="toggle">
            <input
              type="checkbox"
              checked={playit.auto}
              disabled={playitBusy !== null}
              onchange={(e) =>
                playitStep("設定中", () => playitSetAuto(e.currentTarget.checked))}
            />
            <span>{playit.auto ? "開伺服器時自動開公開位址" : "只在手動按下時開"}</span>
          </label>
        </div>

        <div class="row">
          {#if playit.running}
            <Button disabled={playitBusy !== null} onclick={() => playitStep("停止中", playitStop)}>
              停止隧道
            </Button>
            <span class="hint running">執行中，下面的位址現在可以連。</span>
          {:else}
            <Button
              variant="primary"
              disabled={playitBusy !== null}
              onclick={() => playitStep("啟動中", playitStart)}
            >
              啟動隧道
            </Button>
            <span class="hint">
              {playit.auto
                ? "沒啟動的時候公開位址連不進來，不過下次開伺服器會自己啟動。"
                : "沒啟動的時候，公開位址連不進來。"}
            </span>
          {/if}
        </div>

        {#each playit.tunnels as tunnel (tunnel.id)}
          <div class="row token-row">
            <span class="label">{tunnel.name}</span>
            <code class="token addr">{tunnel.address || "配置中…"}</code>
            {#if tunnel.address}
              <Button onclick={() => navigator.clipboard.writeText(tunnel.address)}>複製</Button>
            {/if}
            {#if confirming === `tunnel:${tunnel.id}`}
              <Button onclick={() => (confirming = null)}>取消</Button>
              <Button
                danger
                disabled={playitBusy !== null}
                onclick={() => (
                  (confirming = null),
                  playitStep("移除中", () => playitDeleteTunnel(tunnel.id))
                )}
              >
                確定移除
              </Button>
            {:else}
              <Button
                disabled={playitBusy !== null}
                onclick={() => (confirming = `tunnel:${tunnel.id}`)}
              >
                移除
              </Button>
            {/if}
            {#if confirming === `tunnel:${tunnel.id}`}
              <span class="hint warn">
                這個位址會馬上失效，朋友存的位址也跟著不能用。伺服器之後要再開公開連線，會拿到不同的新位址。
              </span>
            {/if}
            {#if tunnel.disabledReason}
              <span class="hint warn">{tunnel.disabledReason}</span>
            {/if}
          </div>
        {/each}

        {#each playitWaiting as name (name)}
          <div class="row token-row">
            <span class="label">{name}</span>
            <code class="token addr">playit.gg 還在配置位址…</code>
          </div>
        {/each}

        {#if !playit.tunnels.length && !playitWaiting.length}
          <p class="note no-tunnel">
            還沒有公開位址。開一次伺服器就會自動建好，也可以到伺服器頁的「伺服器設定 → 連線」自己按。
          </p>
        {/if}

        {#each playit.notices as notice (notice)}
          <p class="hint warn">{notice}</p>
        {/each}

        <div class="row">
          {#if confirming === "unlink"}
            <Button onclick={() => (confirming = null)}>取消</Button>
            <Button
              danger
              disabled={playitBusy !== null}
              onclick={() => ((confirming = null), playitStep("解除中", playitUnlink))}
            >
              確定解除
            </Button>
            <span class="hint warn">
              解除後公開位址都會停止，要再用得重新登入 playit.gg。隧道本身還留在你的帳號裡。
            </span>
          {:else}
            <Button disabled={playitBusy !== null} onclick={() => (confirming = "unlink")}>
              解除連結
            </Button>
            <span class="hint">
              只忘掉這台電腦的金鑰。隧道還留在你的 playit.gg 帳號裡，要刪要去他們的網站。
            </span>
          {/if}
        </div>
      {/if}

      {#if playit.offline}
        <p class="hint warn">連不上 playit.gg：{playit.offline}</p>
      {/if}
    {/if}

    {#if playitError}
      <p class="agent-error" role="alert">{playitError}</p>
    {/if}

    <p class="note">
      隧道服務由 <a href="https://playit.gg" target="_blank" rel="noreferrer">playit.gg</a> 提供，
      不是這個程式的一部分；用它就等於接受
      <a href="https://playit.gg/terms" target="_blank" rel="noreferrer">playit.gg 的服務條款</a>。
      代理程式是按下按鈕時才從 playit 官方下載的，帳號是你自己的，免費方案的流量與速度限制由他們決定。
      在意延遲或不想讓流量經過第三方，就改用 Radmin VPN 之類的虛擬網路，讓大家在同一個網段裡直連。
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
            type="text"
            inputmode="numeric"
            class:bad={portBad}
            bind:value={portText}
            disabled={busy}
            onchange={applyPort}
          />
        </label>
      </div>
      {#if portBad}
        <p class="agent-error" role="alert">連接埠要是 1024 到 65535 之間的整數。</p>
      {/if}

      <div class="row token-row">
        <span class="label">配對碼</span>
        <code class="token">{showToken || !agent.enabled ? agent.token : "••••-••••-••••-••••"}</code>
        <Button onclick={() => (showToken = !showToken)}>
          {showToken ? "隱藏" : "顯示"}
        </Button>
        <Button onclick={() => navigator.clipboard.writeText(agent!.token)}>複製</Button>
        {#if confirming === "regen"}
          <Button onclick={() => (confirming = null)}>取消</Button>
          <Button danger onclick={regenerate} disabled={busy}>確定換一組</Button>
        {:else}
          <Button onclick={() => (confirming = "regen")} disabled={busy}>換一組</Button>
        {/if}
      </div>
      {#if confirming === "regen"}
        <p class="hint warn">舊的配對碼會立刻失效，已經連上的朋友要重新輸入新的。</p>
      {/if}

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
</section>

<Modal bind:open={reportOpen} title="回報與建議" width={560}>
  <div class="report">
    <label class="field">
      <span>一句話說明</span>
      <input
        bind:value={reportTitle}
        placeholder="例：關掉視窗後伺服器沒有留在系統匣"
        maxlength="120"
      />
    </label>

    <label class="field">
      <span>詳細描述</span>
      <textarea
        bind:value={reportBody}
        rows="10"
        spellcheck="false"
        placeholder={"發生了什麼？\n你原本預期會怎樣？\n怎麼重現？\n\n如果是建議：想要什麼功能，現在是怎麼將就的？"}
      ></textarea>
    </label>

    {#if reportError}
      <p class="report-error" role="alert">{reportError}</p>
    {/if}
  </div>

  {#snippet footer()}
    <span class="count" class:over={reportLength > REPORT_MAX}>
      {reportLength} / {REPORT_MAX}
    </span>
    <Button onclick={() => (reportOpen = false)}>取消</Button>
    <Button variant="primary" onclick={sendReport} disabled={!reportReady}>
      在 GitHub 開啟
    </Button>
  {/snippet}
</Modal>

<style>
  .report {
    display: flex;
    flex-direction: column;
    gap: var(--gap-field);
  }

  .report .field {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .report .field > span {
    color: var(--muted);
    font-size: var(--font-small);
  }

  .report input,
  .report textarea {
    padding: 9px 11px;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-input);
    color: var(--fg);
    font: inherit;
    font-size: var(--font-body);
    line-height: 1.7;
    resize: none;
  }

  .report input:focus,
  .report textarea:focus {
    border-color: var(--accent);
  }

  .report-error {
    color: var(--error);
    font-size: var(--font-small);
  }

  /* Sits at the left of the footer, so the two buttons stay where the eye
     already expects them. */
  .count {
    flex: 1;
    color: var(--faint);
    font-size: var(--font-small);
    font-variant-numeric: tabular-nums;
  }

  .count.over {
    color: var(--error);
  }

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

  .port input.bad {
    border-color: var(--error);
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

  /* The sentence beside a button that says what pressing it costs. Sits on the
     same line, so it is read before the click rather than after. */
  .hint {
    flex: 1;
    font-size: var(--font-tiny);
    color: var(--faint);
    line-height: 1.6;
  }

  .hint.running {
    color: var(--accent);
  }

  .hint.warn {
    color: var(--error);
  }

  /* A hostname is read left to right and can be long — no letter-spacing, and
     it may shrink rather than push the copy button off the row. */
  .addr {
    flex: 0 1 auto;
    min-width: 0;
    letter-spacing: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .note.no-tunnel {
    border-top: none;
    padding-top: 0;
  }

  .note a {
    color: var(--accent);
  }
</style>
