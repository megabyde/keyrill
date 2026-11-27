# Keyrill

[Русская версия](README.ru.md)

Keyrill fixes English/Russian keyboard-layout mistakes as you type: `ghbdtn` becomes `привет`,
`руддщ` becomes `hello`. It runs offline and is available as an alpha for macOS and Windows. Linux
is planned.

[Install on macOS](#macos-setup) · [Install on Windows](#windows-setup)

## How it works

Select a Keyrill input source for English or Russian. With automatic correction enabled, Space
checks the word you just typed and converts it when the evidence points to the other layout. Keyrill
uses a built-in frequency model and protects words in your personal dictionaries. It does not log,
save or send what you type.

| Action | macOS | Windows |
| -- | -- | -- |
| Convert the current word manually | Control+Option+K | Left Ctrl+Left Alt+K |
| Reverse the correction immediately | Control+Option+Z | Left Ctrl+Left Alt+Z |
| Restore the original and finish the marked word | Escape | Escape |

The shortcuts work on the current word in supported fields. On Windows, use the physical K and Z
keys even when typing in the Russian layout.

### macOS

Select **Keyrill EN** or **Keyrill RU**. The input menu shows whether correction is ready.
**Correction in This App** changes the mode for the current application. Source switching after
automatic correction can be turned off in Settings.

- **Composition** (TextEdit, Safari, Firefox) edits only the marked word after checking its text,
  range and caret. See the [composition edit contract](docs/macos.md#composition).
- **Compatibility** (terminals, editors, Microsoft Office and other unlisted applications) uses
  simulated Backspaces and typing. Unlisted apps default to manual correction, with automatic
  correction available in Settings. This mode needs Accessibility and Input Monitoring. Typing
  during a rewrite, focus changes, autocompletion and terminal interfaces can lose keystrokes or
  corrupt the word. See [ADR 006](docs/adr/006-compatibility-backend.md).
- **Off** passes keys through in excluded apps such as password managers and remote desktops.

The [macOS application tiers](docs/macos.md#application-tiers) list the built-in defaults.

### Windows

Select **Keyrill English** or **Keyrill Русский**. Automatic correction starts off. To turn it on,
select **Enable automatic correction in admitted controls** in Keyrill Settings.

Windows correction was verified in the main editing field of Notepad 11.2609.11.0 x64. Fields
outside the supported set pass input through. See the
[Windows application checks](docs/windows.md#application-qualification) for tested versions, fields
and results.

## Install from source

### macOS setup

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

1. In System Settings > Keyboard > Input Sources, add **Keyrill EN** and **Keyrill RU** and select
   one of them. A newly installed input method sometimes appears only after logging out and back in.

1. Open **Settings…** from the Keyrill input menu, enable automatic correction, and check the
   English and Russian dictionary files. The defaults are `~/.vim/spell/en.utf-8.add` and
   `~/.vim/spell/ru.utf-8.add`, and a missing file at a default path counts as empty, so you can
   leave them if you don't use Vim. A missing file at a path you chose keeps automatic correction
   off, so a typo there doesn't silently drop your protected words. **Use words added with Learn
   Spelling** is off by default. To protect those words too, click **Choose…** beside the checkbox,
   select Apple's `LocalDictionary` in the folder the picker opens, then click **Save**. macOS may
   grant access when the file is selected; Cancel in Settings does not revoke access already
   granted. Check the input menu for read failures. If access is lost after a restart or update,
   choose the file again. Without access, correction works without those words.

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

### Windows setup

Use an x64 Windows host with the
[build prerequisites](docs/windows.md#build-and-check-without-installation). The package includes
TSF adapters for 64-bit and 32-bit applications.

1. [Build and verify the package](docs/windows-package.md#candidate-preparation).
1. Add the standard U.S. and Russian keyboard layouts, then
   [install the package as administrator](docs/windows-package.md#explicit-maintainer-commands).
1. Enable and select a Keyrill input profile in Windows.

## Known limitations on macOS

- Secure Input stops correction, but Keyrill doesn't detect every password or hidden terminal
  prompt. With Secure Input off, compatibility mode can hold sensitive typing in memory.
- Only the word being typed can be corrected: the mark in composition, the word Keyrill tracks in
  compatibility mode. An arbitrary committed range or a selection can't be converted.
- In English mode, the keys that type `[ ] ; ' , .` and `\` count as letters, because they are х, ъ,
  ж, э, б, ю and ё in Russian: `[jhjij` becomes `хорошо`. While the word is marked, an editor's
  smart quotes and automatic bracket pairing may not apply to them.
- In raw-key terminals, the manual shortcuts can reach the terminal as well.

## Documentation

The same documents, with the Rust API, are published at <https://megabyde.github.io/keyrill/>.

- [How Keyrill works](docs/how-it-works.md): one word from key press to verified edit; start here
- [Architecture](ARCHITECTURE.md): components, state ownership and safety invariants
- [Engine](docs/engine.md): mapping, inference, personal vocabulary and model evaluation
- [macOS adapter](docs/macos.md): InputMethodKit integration, settings and build variants
- [Windows adapter](docs/windows.md): TSF integration, verification and application coverage
- [Porting](docs/porting.md): the requirements for native adapters, with the Windows work as an
  example
- [Learning Rust and Objective-C++](docs/learn/README.md): the languages taught on this code, with
  exercises
- [Development](docs/development.md) and [testing](docs/testing.md)
- [Decisions](docs/adr/) and the [glossary](docs/glossary.md)

## License

Keyrill's source is [MIT-licensed](LICENSE). The language model is derived from wordfreq data under
CC BY-SA 4.0, and the evaluation corpora carry their own licenses; MIT does not relicense them. See
[assets](assets/README.md) and [model](model/README.md).
