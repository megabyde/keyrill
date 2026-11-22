# Windows admission stability

Editing requires a compiled bounded field contract for the proof editor or an exact supported
Notepad version and native main-field hierarchy. Ordinary TSF scope alone supplies no field
semantics and cannot grant editing authority. Generic hosts, including browser widgets, Word,
terminal wrappers and unknown Notepad versions, pass input without character reads, ACP access or
composition effects. Application settings cannot grant admission.

Within an admitted bounded field, each operation requires fresh identity, status and scope
validation. The bounded contract supplies ordinary-field authority under its qualified host
assumptions. It does not grant permission to every control in the application.

## Documented guarantees

An editing session grants text and property access and excludes competing text-service locks. A
read/write request requires a writable document. The application must preserve document content
throughout its granted lock. These rules establish the document-access contract used by the
executor.
[RequestEditSession](https://learn.microsoft.com/en-us/windows/win32/api/msctf/nf-msctf-itfcontext-requesteditsession),
[RequestLock](https://learn.microsoft.com/en-us/windows/win32/api/textstor/nf-textstor-itextstoreacp-requestlock).

Dynamic status and context/thread compartment changes have notification interfaces. The executor
requires status and all three relevant compartment subscriptions before requesting an editing
session. Each notification revokes its frame through an atomic token. Missing advice refuses
inspection. Uncertain registration or cleanup terminates the pending authority. Every acquired
cookie is surrendered once. Callback objects own their revocation token and DLL lease through the
last host reference.
[ITfStatusSink](https://learn.microsoft.com/en-us/windows/win32/api/msctf/nf-msctf-itfstatussink-onstatuschange),
[compartment notifications](https://learn.microsoft.com/en-us/windows/win32/tsf/compartments).

Thread-manager context and focus callbacks invalidate the service generation. Independent providers
retain the actually advised sink and use it when changing context. Tests compare character reads,
composition starts, character writes, and key disposition at every reached metadata boundary.
[OnPushContext](https://learn.microsoft.com/en-us/windows/win32/api/msctf/nf-msctf-itfthreadmgreventsink-onpushcontext),
[OnSetFocus](https://learn.microsoft.com/en-us/windows/win32/api/msctf/nf-msctf-itfthreadmgreventsink-onsetfocus).

## Final operation boundary

`permit_range` completes callback-capable focus, context, view, range, status, and compartment
queries before reading the ordinary scope. Its final field comparison recaptures Win32 metadata for
the known HWND. The permit retains the scope's property and interface leases and returned allocation
throughout the operation, including every foreign reference release afterward.

Character reads, composition starts, replacements, selection changes, and composition ends each
retain a named permit. Revocation after a character write stops further character reads and produces
an unknown outcome with consumed input.

## Bounded host assumptions

Application-provided input scope enters TSF through host attributes. Attribute-change notifications
describe changes to that data. Text-edit notifications arrive after the editing session. The current
session's first character read relies on the current property and scope values.
[GetAttribute](https://learn.microsoft.com/en-us/windows/win32/api/msctf/nf-msctf-itfcontextowner-getattribute),
[OnAttributeChange](https://learn.microsoft.com/en-us/windows/win32/api/msctf/nf-msctf-itfcontextownerservices-onattributechange),
[OnAttrsChange](https://learn.microsoft.com/en-us/windows/win32/api/textstor/nf-textstor-itextstoreacpsink-onattrschange),
[OnEndEdit](https://learn.microsoft.com/en-us/windows/win32/api/msctf/nf-msctf-itftexteditsink-onendedit).

The admitted bounded host returns an accurate property for the requested range and an accurate
current scope from its input-scope interface during the granted property access. Dynamic status,
compartment, and context changes notify the installed sinks before the pending operation resumes.
This coherence and notification-before-resume requirement is an additional supported-host assumption
inferred from the API contracts. Its timing needs qualification on each actual host. Microsoft
documents the returned values and their access requirements.
[GetValue](https://learn.microsoft.com/en-us/windows/win32/api/msctf/nf-msctf-itfreadonlyproperty-getvalue),
[GetInputScopes](https://learn.microsoft.com/en-us/windows/win32/api/inputscope/nf-inputscope-itfinputscope-getinputscopes).

Unsupported provider behavior includes stale scope values, silent status or top-context mutation
that omits its notification, and arbitrary policy mutation inside the injected metadata capture.
Production capture uses Win32 metadata getters. An unobserved violation can precede the first read.
Finite repeated queries retain that limitation.

Controlled schedules cover every reached getter and release boundary for scope revocation, plus
every reached metadata boundary for notified status, compartment, and context changes. The external
silent-mutation reproducer retains counterexamples for the unsupported behaviors above. Application
qualification collects independent UIA public-document and caret observations alongside TSF
lifecycle evidence on actual application builds. The containment candidate still requires installed
denied-browser and working bounded-Notepad checks. Full Windows application support remains open.

## Browser payment field counterexample

The installed candidate `940ee5df` fails the sensitive-field exclusion gate in Edge 157.0.4341.60
x64. A generated `g` in a local `type=text autocomplete=cc-number` fixture entered a granted TSF
session and created an owned composition with one verified write. The fixture contained only public
test data. A separate readonly field denied the session, and a password field refused at the
disabled-context check. Neither created a composition or attempted a write. The browser used the
same native window for the logical fields.

In the reviewed upstream Chromium revision, Autofill recognizes the payment token. The IME type
still comes from the form-control type, and the input-scope property uses that type, the input mode,
and the learning preference. This path does not pass the payment token through to TSF. The source
predicts ordinary default scope for the fixture. The Edge runtime journal proves that Keyrill
admitted the field, but does not identify the raw scope value returned by the tested Edge binary.
[Autofill token mapping](https://chromium.googlesource.com/chromium/src/+/bc7da50d38d0bde911cb7762c7c418f8a2708730/components/autofill/core/common/autocomplete_parsing_util.cc#40),
[Blink IME type](https://chromium.googlesource.com/chromium/src/+/bc7da50d38d0bde911cb7762c7c418f8a2708730/third_party/blink/renderer/core/editing/ime/input_method_controller.cc#1765),
[TSF property creation](https://chromium.googlesource.com/chromium/src/+/bc7da50d38d0bde911cb7762c7c418f8a2708730/ui/base/ime/win/tsf_text_store.cc#932).

The bounded-only containment removes generic editing authority. It refuses both ordinary and
sensitive browser fields because their measured native identity and existing TSF metadata do not
establish the required distinction. The unknown-Notepad observer remains read-only and scope-only. A
completed ordinary-scope observation reports `Unsupported` with `acp=0`; retired wire values remain
reserved. Observation never supplies editing authority.

Browser correction remains unimplemented. Restoring it requires authoritative semantic field
identity and revision bound to the actual native TSF context and range, with synchronous revocation.
Native window identity, app selection and an ordinary scope cannot substitute for that contract.
Future qualification must include same-context ordinary/payment distinctions, A-B-A changes and
silent provider mutation. No finite metadata-query loop proves those cases safe.
