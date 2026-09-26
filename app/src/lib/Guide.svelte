<!--
  First-run walkthrough.

  Six steps, shown once. Not a help page: a help page is something you have to
  decide to go looking for, and the moment someone needs this is the moment the
  window opens for the first time and nothing on screen has a server in it.

  Deliberately not a tour with arrows pointing at live elements — an overlay
  anchored to real components breaks the first time one of them moves, and
  there is nothing to point at yet on an empty install.
-->
<script lang="ts">
  import Button from "./Button.svelte";
  import Modal from "./Modal.svelte";

  let { open = $bindable(false) }: { open: boolean } = $props();

  /** Title, body, and the one thing to actually do at this step. */
  const STEPS: { title: string; body: string; act?: string }[] = [
    {
      title: "歡迎",
      body: "這個程式幫你在自己的電腦上開 Minecraft Forge 伺服器：下載、安裝、啟動、看主控台、裝模組、改設定，都在這裡。\n\n接下來六頁講一遍完整流程，一分鐘看完。",
    },
    {
      title: "一、下載 Forge",
      body: "先到「下載」頁，選 Minecraft 版本，再選一個 Forge build（清單最上面的是最新的），按下載。\n\n下載回來的是安裝檔，還不是伺服器本身。",
      act: "下載頁 → 選版本 → 下載",
    },
    {
      title: "二、建立伺服器",
      body: "回「主頁」，按「新增伺服器」，取個名字，挑剛才下載的安裝檔。\n\n安裝會跑一兩分鐘，過程中的輸出會即時顯示。這一步也代表你接受 Minecraft 的 EULA。\n\n已經有現成的伺服器資料夾？用「匯入」指過去，檔案不會被搬動，之後刪除也只是從清單移除。",
      act: "主頁 → 新增伺服器",
    },
    {
      title: "三、Java",
      body: "不同 Minecraft 版本要不同的 Java：1.20 要 17、1.21 以上要 21、26 以上要 25。\n\n不用自己先裝。缺哪個版本，程式會在你按啟動時跳出來問，按一下就下載好並且只給這個伺服器用，不會動到你電腦本來的 Java。",
    },
    {
      title: "四、啟動與主控台",
      body: "點卡片進伺服器頁，按「啟動」。看到 Done 那行就是開好了。\n\n下面那格可以直接打指令，跟在伺服器主機打字一樣：op、gamemode、weather 都行，不用加斜線。\n\n「停止」是正常關機，世界會存檔。\n\n伺服器開著的時候，主控台上方會列出線上玩家，滑到名字上就能給 OP、踢出或封鎖。\n\n如果伺服器意外結束，同一個位置會說明原因，並給一顆直接跳到問題設定的按鈕。\n\n伺服器還在跑的時候按視窗的 ✕，程式會縮到系統匣繼續看著它，不會把伺服器關掉。要整個結束就在系統匣圖示按右鍵選「安全關閉並離開」。",
      act: "伺服器頁 → 啟動",
    },
    {
      title: "五、設定與模組",
      body: "伺服器頁的「設定」分頁分成連線、玩法、世界、進階四區，server.properties 裡的每一個項目都在裡面，中文名下面就是原始的鍵名。\n\n「模組」把 jar 丟進去就好。「原始檔」可以直接編輯三個檔案。\n\n設定和模組都要伺服器停著才能改——執行中改的東西會在關機時被蓋掉。",
      act: "伺服器頁 → 設定",
    },
    {
      title: "六、讓別人連進來",
      body: "同一個區域網路：把你的內網 IP 給他們就行。\n\n不同地方：到「設定 → 公開連線」按一顆「開啟公開連線」，在 playit.gg 註冊一次，之後每次開伺服器都會自動有一個公開網址。伺服器頁上方有「複製公開位址」，複製丟給朋友，他們貼進「多人遊戲 → 直接連線」就能進來。不用設定路由器的通訊埠轉發，也不用管你家是不是真實 IP；代價是連線會經過 playit.gg 的伺服器。\n\n不想讓流量經過第三方：用 Radmin VPN 或 Hamachi 之類的工具讓大家在同一個虛擬網路裡。\n\n連不進來時先確認伺服器是「執行中」，再確認防火牆有放行 Java。\n\n這份說明之後可以在「設定」頁重新打開。",
      act: "設定 → 公開連線",
    },
  ];

  let step = $state(0);
  const last = $derived(step === STEPS.length - 1);

  function close() {
    open = false;
    step = 0;
  }
</script>

<Modal bind:open title={STEPS[step].title} width={520}>
  <p class="body">{STEPS[step].body}</p>

  {#if STEPS[step].act}
    <p class="act">{STEPS[step].act}</p>
  {/if}

  {#snippet footer()}
    <span class="dots" aria-label="第 {step + 1} 頁，共 {STEPS.length} 頁">
      {#each STEPS as s, i (s.title)}
        <span class="dot" class:on={i === step}></span>
      {/each}
    </span>
    <Button onclick={close}>略過</Button>
    {#if step > 0}
      <Button onclick={() => (step -= 1)}>上一步</Button>
    {/if}
    <Button variant="primary" onclick={() => (last ? close() : (step += 1))}>
      {last ? "開始使用" : "下一步"}
    </Button>
  {/snippet}
</Modal>

<style>
  /* The line breaks are the paragraph breaks — `white-space: pre-line` keeps
     the copy above readable as prose instead of a pile of <p> tags. */
  .body {
    color: var(--fg);
    font-size: var(--font-body);
    line-height: 1.9;
    white-space: pre-line;
  }

  /* The one thing to do on this step, as a path through the app rather than a
     sentence — it is what someone re-reads after closing the guide. */
  .act {
    margin-top: 16px;
    padding-top: 12px;
    border-top: 1px solid var(--border);
    color: var(--accent);
    font-size: var(--font-small);
  }

  .dots {
    flex: 1;
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .dot {
    width: 5px;
    height: 5px;
    border-radius: var(--radius-pill);
    background: var(--border);
  }

  .dot.on {
    background: var(--muted);
  }
</style>
