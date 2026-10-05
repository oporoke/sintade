import { ChangeDetectionStrategy, Component } from '@angular/core';

/**
 * The privacy policy (a public page, `/privacy`). The browser extension's store listings link to
 * it. DRAFT: facts below describe what the product does today; the bracketed items and the legal
 * wording need a Tanzanian advocate's review before launch (docs/design.md §19 "Legal documents").
 */
@Component({
  selector: 'app-privacy-page',
  changeDetection: ChangeDetectionStrategy.OnPush,
  styles: `
    :host {
      display: block;
      max-width: 760px;
      margin: 0 auto;
      padding: var(--space-4) var(--space-3);
      line-height: 1.55;
    }
    h2 {
      margin-top: var(--space-4);
    }
    .draft {
      border: 1px solid var(--color-border);
      border-radius: var(--radius-md);
      padding: var(--space-2) var(--space-3);
      color: var(--color-text-muted);
    }
    table {
      border-collapse: collapse;
      width: 100%;
    }
    th,
    td {
      text-align: left;
      vertical-align: top;
      border-bottom: 1px solid var(--color-border);
      padding: var(--space-2);
    }
  `,
  template: `
    <h1 data-testid="privacy-title" i18n>Privacy policy</h1>
    <p class="draft" data-testid="privacy-draft" i18n>
      Draft for legal review. Items in [square brackets] are still to be completed.
    </p>
    <p i18n>
      Last updated: [date]. This policy explains what Sintade collects when you use the web app or
      the Sintade browser extension, why, and what you can do about it.
    </p>

    <h2 i18n>Who we are</h2>
    <p i18n>
      Sintade is operated by [company legal name], registered in the United Republic of Tanzania
      ([registration number], [address]). Contact: [privacy contact email].
    </p>

    <h2 i18n>What we collect</h2>
    <ul>
      <li i18n>
        <strong>Your account:</strong> email address, display name, and a salted hash of your
        password (never the password itself). Sign-in cookies that keep you signed in.
      </li>
      <li i18n>
        <strong>Your recordings:</strong> the video, and the audio you choose to include (the tab's
        sound, your microphone), uploaded while you record, plus a title, length and size. These are
        stored so that you can watch, share and download them.
      </li>
      <li i18n>
        <strong>Sharing settings:</strong> who each recording is shared with (only you, your
        workspace, anyone with the link, public), whether downloads are allowed, and link expiry.
      </li>
      <li i18n>
        <strong>Technical data:</strong> your IP address and request details in server logs and rate
        limits, used to keep the service secure and working. We do not log passwords, tokens or the
        contents of recordings.
      </li>
    </ul>
    <p i18n>
      We do not sell your data, show advertising, or use your recordings to train machine-learning
      models [confirm].
    </p>

    <h2 i18n>The browser extension</h2>
    <p i18n>
      The extension exists for one purpose: to record the browser tab you choose and send it to your
      Sintade account. It does this only after you click its button and press Record. It does not
      read, record or send anything about other tabs or your browsing history, and it runs no
      analytics.
    </p>
    <table data-testid="privacy-permissions">
      <thead>
        <tr>
          <th i18n>Permission</th>
          <th i18n>Why</th>
        </tr>
      </thead>
      <tbody>
        <tr>
          <td><code>activeTab</code></td>
          <td i18n>
            Lets the extension act on the tab you are looking at when you click it, and nothing
            else.
          </td>
        </tr>
        <tr>
          <td><code>tabCapture</code></td>
          <td i18n>Records that tab's picture and sound.</td>
        </tr>
        <tr>
          <td><code>offscreen</code></td>
          <td i18n>
            Keeps a hidden page open while recording, because the extension's background process
            cannot record video.
          </td>
        </tr>
        <tr>
          <td><code>scripting</code></td>
          <td i18n>
            Draws the click highlights and (if you turn it on) the keystroke labels on the tab you
            are recording, so they appear in the video. Keys typed into password, one-time-code and
            card-number fields are never shown.
          </td>
        </tr>
        <tr>
          <td><code>storage</code></td>
          <td i18n>
            Remembers your choices (tab audio, microphone, highlights, keystrokes) and the state of
            the current recording on your device.
          </td>
        </tr>
        <tr>
          <td i18n>Access to the Sintade website</td>
          <td i18n>
            Lets the extension use your existing Sintade sign-in to create and upload recordings. It
            never sees or stores your password.
          </td>
        </tr>
      </tbody>
    </table>
    <p i18n>
      The extension sends data only to Sintade, and only your recording and the requests needed to
      upload it and create its link. It downloads no code from anywhere: everything it runs ships
      inside the extension. The microphone is used only if you turn it on, after your browser has
      asked you.
    </p>

    <h2 i18n>Who can see your recordings</h2>
    <p i18n>
      You decide. A recording is visible to the people your link settings allow. Anyone with a
      "link" URL can watch and pass it on; revoke or change the link to stop new viewing. Viewers
      get short-lived (15 minute) addresses to the video. If you allow downloads, viewers may keep
      copies we cannot recall.
    </p>

    <h2 i18n>Who we share data with</h2>
    <p i18n>
      Service providers that run Sintade for us, under contract: [hosting and storage provider],
      [email delivery provider]. We disclose data to authorities only when the law requires it.
    </p>

    <h2 i18n>How long we keep it</h2>
    <ul>
      <li i18n>
        Recordings stay until you delete them. Deleted recordings sit in the trash for 30 days, then
        are erased, including their files.
      </li>
      <li i18n>Pieces of an unfinished upload are erased after 7 days.</li>
      <li i18n>Backups are kept for [14] days and then expire.</li>
      <li i18n>When you delete your account, your data is erased within 30 days.</li>
    </ul>

    <h2 i18n>Your rights</h2>
    <p i18n>
      Under Tanzania's Personal Data Protection Act, 2022 (and, where it applies, the GDPR) you may
      ask to see, correct, export or delete your data, and object to or restrict some uses. Write to
      [privacy contact email]; we reply within 30 days. You may also complain to the Personal Data
      Protection Commission.
    </p>

    <h2 i18n>Security</h2>
    <p i18n>
      Connections are encrypted. Videos are stored privately and reached only through short-lived
      signed addresses. Access is checked on every request, and one workspace can never reach
      another's recordings.
    </p>

    <h2 i18n>Children</h2>
    <p i18n>Sintade is for people aged 18 or over, and is not directed at children.</p>

    <h2 i18n>Changes</h2>
    <p i18n>
      We will post changes here and, for material changes, tell you by email before they apply.
    </p>
  `,
})
export class PrivacyPage {}
