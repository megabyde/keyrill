# Keyrill

Keyrill fixes English/Russian keyboard-layout mistakes as you type: `ghbdtn` becomes `привет`,
`руддщ` becomes `hello`. It is a local input method with a Rust engine, a versioned C ABI and a
native macOS InputMethodKit adapter. Inference runs offline against a frozen frequency model and
your own spelling lists: Vim `.add` files if you keep them and the words you taught macOS with Learn
Spelling. Nothing is logged, persisted or sent over the network.

> [!NOTE]
> This is an alpha. Only macOS has an adapter. The engine and ABI are platform-neutral, and Windows
> and Linux adapters are open work; see [porting](docs/porting.md).

## How it works

You select **Keyrill English** or **Keyrill Russian** as the input source in place of the stock U.S.
and Russian layouts. When you press Space, Keyrill decides whether the word you just typed was meant
for the other layout and, if the evidence is strong enough, replaces it. Unknown, technical,
mixed-script, mixed-case and personally protected words stay as typed: the engine prefers a missed
correction to a wrong one.

| Action | Key |
| -- | -- |
| Convert the current word regardless of the model | Control+Option+K |
| Undo a correction, right after it and before the next key | Control+Option+Z |
| Restore the original and keep it | Escape, while the word is still marked |

The input menu lists both shortcuts and leads with one line saying whether automatic correction is
ready in the current application, and if not, why.

After an automatic correction, Keyrill switches to the input mode that matches the corrected
language, so the next word is typed in the right layout; undoing the correction switches back. Turn
this off in Settings if you prefer to switch yourself.

Keyrill edits text in one of three ways, chosen per application when it gains focus:

- **Composition** (TextEdit, Safari, Firefox): the word stays marked (underlined) while you type,
  and Keyrill replaces only that mark after checking its exact text, range and caret. If a check
  fails, Keyrill commits the marked text as the application reports it and leaves that word alone;
  nothing is guessed or replayed. Correction resumes with the next word; after a second failure it
  stays off in that field until you switch away and back.
- **Compatibility** (terminals, code editors, Microsoft Office and every application not listed):
  Control+Option+K and Control+Option+Z rewrite the word by posting Backspaces and the replacement.
  Automatic correction is off here unless you turn it on for unlisted applications in Settings.
  Keyrill cannot see what the application actually deleted or inserted, so typing during a rewrite,
  focus changes, autocompletion and TUIs can lose keystrokes or corrupt the word. This mode needs
  Accessibility and Input Monitoring access. See [ADR 006](docs/adr/006-compatibility-backend.md)
  for the trade-off.
- **Off** (password managers, remote desktops, virtual machines): keys pass through untouched.

The input menu shows the current application's mode; **Correction in This App** changes it. The
built-in lists are in [macOS adapter](docs/macos.md#application-tiers).

## Install from source

You need Apple Silicon macOS with Command Line Tools and Rust (rustup installs the exact toolchain
from `rust-toolchain.toml` on first use).

1. Build, sign and copy the alpha bundle to your Input Methods folder:

   ```console
   $ make install
   ```

   The bundle is signed ad hoc, which gives every build a different code requirement, so macOS may
   ask for the compatibility permissions again after each update. Create a self-signed code-signing
   certificate in Keychain Access and pass its name to keep the requirement stable:
   `make install SIGN_IDENTITY="Keyrill Local"`. `make bundle` builds and signs
   `target/alpha.noindex/KeyrillAlpha.app` without installing it.

1. In System Settings > Keyboard > Input Sources, add **Keyrill English** and **Keyrill Russian**
   and select one of them. A newly installed input method sometimes appears only after logging out
   and back in.

1. Open **Settings…** from the Keyrill input menu, enable automatic correction, and check the
   English and Russian dictionary files. The defaults are `~/.vim/spell/en.utf-8.add` and
   `~/.vim/spell/ru.utf-8.add`, and a missing file at a default path counts as empty, so you can
   leave them if you don't use Vim. A missing file at a path you chose keeps automatic correction
   off, so a typo there doesn't silently drop your protected words. Words you add with Learn
   Spelling in any app are protected as well.

1. For compatibility mode, grant Keyrill Alpha access under Privacy & Security > Accessibility and
   Input Monitoring. Keyrill checks both without prompting when a compatibility application is
   active, and the input menu shows what is missing. Without them, those applications get plain
   typing and no correction.

To update, select stock U.S. and run `make install` again, with the same `SIGN_IDENTITY`. It quits
the running server before replacing the bundle, because copying over the bundle of a running server
can get it killed for an invalid code signature.

The new server starts when you select a Keyrill mode again. Settings are kept.

To uninstall, remove both modes from Input Sources, quit the server, delete
`~/Library/Input Methods/KeyrillAlpha.app` and, if you want the settings gone too, run
`defaults delete org.keyrill.inputmethod.alpha`. Keyrill never modifies your dictionary files.

## Known limitations

- Secure Input stops correction, but Keyrill doesn't detect every password or hidden terminal
  prompt. With Secure Input off, compatibility mode can hold sensitive typing in memory.
- Only the word being typed can be corrected. Already committed text and selections can't be
  converted.
- In English mode, the keys that type `[ ] ; ' , .` and `\` count as letters, because they are х, ъ,
  ж, э, б, ю and ё in Russian: `[jhjij` becomes `хорошо`. While the word is marked, an editor's
  smart quotes and automatic bracket pairing may not apply to them.
- Only Space triggers automatic correction, so a word followed directly by punctuation or Return
  stays as typed; Control+Option+K still converts it.
- In raw-key terminals, Control+Option+K and Control+Option+Z can reach the terminal as well.
- Typing whole test-split sentences in the wrong layout, automatic correction fixes about 46% of
  Russian and 60% of English tokens, with no false corrections on the same sentences typed
  correctly; see [engine](docs/engine.md#evaluation).

## Documentation

The same documents, with the Rust API, are published at <https://megabyde.github.io/keyrill/>.

- [How Keyrill works](docs/how-it-works.md): one word from key press to verified edit; start here
- [Architecture](ARCHITECTURE.md): components, state ownership and safety invariants
- [Engine](docs/engine.md): mapping, inference, personal vocabulary and the model
- [macOS adapter](docs/macos.md): InputMethodKit integration, settings and build variants
- [Porting](docs/porting.md): what a Windows or Linux adapter must implement
- [Development](docs/development.md) and [testing](docs/testing.md)
- [Decisions](docs/adr/)

## License

Keyrill's source is [MIT-licensed](LICENSE). The language model is derived from wordfreq data under
CC BY-SA 4.0, and the evaluation corpora carry their own licenses; MIT does not relicense them. See
[assets](assets/README.md) and [model](model/README.md).
