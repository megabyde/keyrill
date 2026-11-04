//! Committed-input tracking for the explicitly weaker synthetic backend.

use super::*;

/// Plan against bounded observed keystrokes, never a document snapshot.
///
/// # Safety
/// The handle, event and output obey the aligned, live, nonaliasing C object contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kr_plan_compatibility_event(
    pointer: *mut Engine,
    event: *const NativeEvent,
    automatic: u32,
    out: *mut CompatibilityPlan,
) -> u32 {
    if event.is_null() || out.is_null() {
        return NULL;
    }
    // SAFETY: the caller supplies complete input/output objects; all data is copied
    unsafe {
        with_engine(pointer, |engine| {
            if automatic > 1 {
                return BAD_TAG;
            }
            let (mut input, mode, _) = match decode_event(&*event) {
                Ok(decoded) => decoded,
                Err(error) => return error,
            };
            if engine.backend == Backend::Composition {
                return STATE;
            }
            // Escape belongs to the host here: there is no owned document mark to restore
            if input == Event::Escape {
                input = Event::Finish;
            }
            let mut word_boundary = if engine.session.has_word() {
                engine.compatibility_word_boundary
            } else {
                engine.compatibility_boundary
            };
            let context = if engine.compatibility_collect {
                Context::Boundary
            } else {
                Context::Ineligible
            };
            // Manual targets are observed runs; automatic inference needs an observed Space
            // before the run. A reset/focus event supplies no document-boundary evidence.
            let mut plan = match propose(
                engine,
                &input,
                mode,
                context,
                automatic != 0 && word_boundary,
            ) {
                Ok(plan) => plan,
                Err(error) => return status(error),
            };
            if plan.disposition == Disposition::Replan {
                // Finishing this virtual word dispatches nothing: its input already passed
                if let Err(error) = engine.session.acknowledge(plan.id, Outcome::Verified) {
                    return status(error);
                }
                word_boundary = engine.compatibility_boundary;
                plan = match propose(
                    engine,
                    &input,
                    mode,
                    Context::Boundary,
                    automatic != 0 && word_boundary,
                ) {
                    Ok(plan) => plan,
                    Err(error) => return status(error),
                };
            }
            let space = matches!(&input, Event::Text(units) if units == &[32]);
            let mut unchanged = plan.expected.clone();
            if space {
                unchanged.push(32);
            }
            let rewrite = plan.action == Action::Mark
                && (space || matches!(input, Event::Manual | Event::Reverse))
                && plan.text != unchanged;
            let disposition = if matches!(input, Event::Manual | Event::Reverse) {
                1
            } else {
                0
            };
            let mut copied = CompatibilityPlan {
                plan_id: plan.id,
                operation: u32::from(rewrite),
                disposition,
                delete_units: if rewrite {
                    // Raw-key clients can forward Space even when IMK consumes it. Let it
                    // pass everywhere and replace the observed word plus that separator.
                    (plan.expected.len() + usize::from(space)) as u32
                } else {
                    0
                },
                replacement_len: if rewrite { plan.text.len() as u32 } else { 0 },
                replacement: [0; 128],
            };
            if rewrite {
                copied.replacement[..plan.text.len()].copy_from_slice(&plan.text);
            }
            let boundary = if matches!(input, Event::Manual | Event::Reverse) {
                engine.compatibility_boundary
            } else {
                space
            };
            engine.backend = Backend::Compatibility;
            let collect = if matches!(input, Event::Manual | Event::Reverse) {
                engine.compatibility_collect
            } else {
                space
            };
            engine.compatibility_pending = Some(CompatibilityPending {
                id: plan.id,
                rewrite,
                boundary,
                word_boundary,
                collect,
            });
            out.write(copied);
            OK
        })
    }
}

/// Acknowledge observation or synthetic dispatch, never verified document text.
///
/// # Safety
/// The handle obeys the live unique serialized C handle contract, or is null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kr_commit_compatibility(
    pointer: *mut Engine,
    token: u64,
    outcome: u32,
) -> u32 {
    // SAFETY: forwarded unique handle contract; no native calls occur under the borrow
    unsafe {
        with_engine(pointer, |engine| {
            if outcome > 3 {
                return BAD_TAG;
            }
            if engine.backend != Backend::Compatibility {
                return STATE;
            }
            let Some(pending) = engine.compatibility_pending else {
                return STALE;
            };
            if token != pending.id {
                return STALE;
            }
            if (outcome == 0 && pending.rewrite) || (outcome == 1 && !pending.rewrite) {
                return STATE;
            }
            let local_outcome = match outcome {
                0 | 1 => Outcome::Verified,
                2 => Outcome::Refused,
                _ => Outcome::Unknown,
            };
            // The reused session confirms a local keystroke record, not native editing authority
            match engine.session.acknowledge(token, local_outcome) {
                Ok(()) => {
                    engine.compatibility_pending = None;
                    engine.compatibility_boundary = pending.boundary;
                    engine.compatibility_word_boundary = pending.word_boundary;
                    engine.compatibility_collect = pending.collect;
                    OK
                }
                Err(error) => status(error),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    unsafe fn plan(
        engine: *mut Engine,
        text: &str,
        role: u32,
        automatic: u32,
    ) -> CompatibilityPlan {
        let mut event = NativeEvent {
            role,
            mode: 1,
            context: 1,
            len: 0,
            units: [0; 8],
        };
        for unit in text.encode_utf16() {
            event.units[event.len as usize] = unit;
            event.len += 1;
        }
        let mut out = std::mem::MaybeUninit::uninit();
        // SAFETY: unique test handle and complete nonaliasing stack objects
        unsafe {
            assert_eq!(
                kr_plan_compatibility_event(engine, &event, automatic, out.as_mut_ptr()),
                OK
            );
            out.assume_init()
        }
    }

    #[test]
    fn observation_dispatch_receipt_and_backend_separation() {
        // SAFETY: one unique handle; stack buffers live through every call and are freed once
        unsafe {
            let mut engine = std::ptr::null_mut();
            assert_eq!(kr_engine_new(ABI, &mut engine), OK);
            assert_eq!(kr_set_automatic_policy(engine, 1), OK);
            for unit in "ghbdtn".chars() {
                let p = plan(engine, &unit.to_string(), 1, 0);
                assert_eq!(
                    (
                        p.operation,
                        p.delete_units,
                        p.replacement_len,
                        p.disposition
                    ),
                    (0, 0, 0, 0)
                );
                assert_eq!(kr_commit_compatibility(engine, p.plan_id, 1), STATE);
                assert_eq!(kr_commit_compatibility(engine, p.plan_id, 0), OK);
            }
            let p = plan(engine, "", 4, 0);
            assert_eq!((p.operation, p.delete_units, p.disposition), (1, 6, 1));
            assert_eq!(
                String::from_utf16(&p.replacement[..p.replacement_len as usize]).unwrap(),
                "привет"
            );
            assert_eq!(kr_commit(engine, p.plan_id, 0), STATE);
            assert_eq!(
                kr_set_dictionary_utf8(engine, 1, std::ptr::null(), 0),
                STATE
            );
            assert_eq!(kr_commit_compatibility(engine, p.plan_id - 1, 1), STALE);
            assert_eq!(kr_commit_compatibility(engine, p.plan_id, 0), STATE);
            assert_eq!(kr_commit_compatibility(engine, p.plan_id, 1), OK);
            let p = plan(engine, " ", 1, 1);
            assert_eq!(p.operation, 0);
            assert_eq!(kr_commit_compatibility(engine, p.plan_id, 0), OK);
            let p = plan(engine, "", 5, 0);
            assert_eq!(p.delete_units, 7);
            assert_eq!(
                String::from_utf16(&p.replacement[..p.replacement_len as usize]).unwrap(),
                "ghbdtn "
            );
            assert_eq!(kr_commit_compatibility(engine, p.plan_id, 3), OK);
            assert_eq!(kr_commit_compatibility(engine, p.plan_id, 1), STALE);
            assert_eq!(kr_reset(engine), OK);
            let event = NativeEvent {
                role: 1,
                mode: 1,
                context: 0,
                len: 1,
                units: [97; 8],
            };
            let mut out = std::mem::MaybeUninit::uninit();
            assert_eq!(kr_plan_event(engine, &event, out.as_mut_ptr()), STATE);
            kr_engine_free(engine);
        }
    }

    #[test]
    fn automatic_opt_in_and_late_reversal_expiry() {
        // SAFETY: isolated handles; no pointer or borrowed input escapes any call
        unsafe {
            for automatic in [0, 1] {
                let mut engine = std::ptr::null_mut();
                assert_eq!(kr_engine_new(ABI, &mut engine), OK);
                assert_eq!(kr_set_automatic_policy(engine, 1), OK);
                let p = plan(engine, " ", 1, automatic);
                assert_eq!(kr_commit_compatibility(engine, p.plan_id, 0), OK);
                for unit in "ghbdtn".chars() {
                    let p = plan(engine, &unit.to_string(), 1, automatic);
                    assert_eq!(kr_commit_compatibility(engine, p.plan_id, 0), OK);
                }
                let p = plan(engine, " ", 1, automatic);
                assert_eq!(p.operation, automatic);
                if automatic == 1 {
                    assert_eq!((p.delete_units, p.disposition), (7, 0));
                    assert_eq!(
                        String::from_utf16(&p.replacement[..p.replacement_len as usize]).unwrap(),
                        "привет "
                    );
                }
                assert_eq!(kr_commit_compatibility(engine, p.plan_id, automatic), OK);
                let p = plan(engine, "h", 1, automatic);
                assert_eq!(p.operation, 0);
                assert_eq!(kr_commit_compatibility(engine, p.plan_id, 0), OK);
                let p = plan(engine, "", 5, automatic);
                assert_eq!(p.operation, 0);
                assert_eq!(kr_commit_compatibility(engine, p.plan_id, 0), OK);
                kr_engine_free(engine);
            }
        }
    }

    #[test]
    fn reset_is_not_an_automatic_boundary_and_refused_dispatch_invalidates() {
        // SAFETY: isolated unique handle and complete stack input/output buffers
        unsafe {
            let mut engine = std::ptr::null_mut();
            assert_eq!(kr_engine_new(ABI, &mut engine), OK);
            assert_eq!(kr_set_automatic_policy(engine, 1), OK);
            for outcome in [2, 3] {
                assert_eq!(kr_reset(engine), OK);
                for unit in "ghbdtn ".chars() {
                    let p = plan(engine, &unit.to_string(), 1, 1);
                    assert_eq!(p.operation, 0);
                    assert_eq!(kr_commit_compatibility(engine, p.plan_id, 0), OK);
                }
                for unit in "ghbdtn".chars() {
                    let p = plan(engine, &unit.to_string(), 1, 1);
                    assert_eq!(kr_commit_compatibility(engine, p.plan_id, 0), OK);
                }
                let p = plan(engine, " ", 1, 1);
                assert_eq!(p.operation, 1);
                assert_eq!(kr_commit_compatibility(engine, p.plan_id, outcome), OK);
                let event = NativeEvent {
                    role: 1,
                    mode: 1,
                    context: 0,
                    len: 1,
                    units: [97; 8],
                };
                let mut out = std::mem::MaybeUninit::uninit();
                assert_eq!(
                    kr_plan_compatibility_event(engine, &event, 1, out.as_mut_ptr()),
                    STATE
                );
            }
            kr_engine_free(engine);
        }
    }

    #[test]
    fn observed_nonseparators_do_not_acquire_identifier_or_punctuation_suffixes() {
        // SAFETY: the handle and local input/output buffers obey the serialized ABI contract
        unsafe {
            let mut engine = std::ptr::null_mut();
            assert_eq!(kr_engine_new(ABI, &mut engine), OK);
            assert_eq!(kr_set_automatic_policy(engine, 1), OK);
            // The tracker classifies letters with the core session, so the English-mode keys
            // that type Russian letters belong to the observed word: a manual rewrite replaces
            // exactly that run, and reversal restores it
            for (token, converted) in [("[jhjij", "хорошо"), ("b[hf", "ихра"), (";bpym", "жизнь")]
            {
                assert_eq!(kr_reset(engine), OK);
                for unit in token.chars() {
                    let p = plan(engine, &unit.to_string(), 1, 1);
                    assert_eq!(p.operation, 0);
                    assert_eq!(kr_commit_compatibility(engine, p.plan_id, 0), OK);
                }
                let typed: Vec<u16> = token.encode_utf16().collect();
                let p = plan(engine, "", 4, 1);
                assert_eq!(
                    (p.operation, p.delete_units, p.replacement_len),
                    (1, typed.len() as u32, typed.len() as u32)
                );
                assert_eq!(
                    p.replacement[..typed.len()].to_vec(),
                    converted.encode_utf16().collect::<Vec<_>>()
                );
                assert_eq!(kr_commit_compatibility(engine, p.plan_id, 1), OK);
                let p = plan(engine, "", 5, 1);
                assert_eq!((p.operation, p.delete_units), (1, typed.len() as u32));
                assert_eq!(
                    &p.replacement[..p.replacement_len as usize],
                    typed.as_slice()
                );
                assert_eq!(kr_commit_compatibility(engine, p.plan_id, 1), OK);
            }
            for token in ["word_count", "x2ghbdtn", "(ghbdtn", "ghbdtn!"] {
                assert_eq!(kr_reset(engine), OK);
                for unit in token.chars() {
                    let p = plan(engine, &unit.to_string(), 1, 1);
                    assert_eq!(p.operation, 0);
                    assert_eq!(kr_commit_compatibility(engine, p.plan_id, 0), OK);
                }
                let p = plan(engine, "", 4, 1);
                assert_eq!((p.operation, p.delete_units), (0, 0));
                assert_eq!(kr_commit_compatibility(engine, p.plan_id, 0), OK);
                let p = plan(engine, " ", 1, 1);
                assert_eq!(kr_commit_compatibility(engine, p.plan_id, 0), OK);
                for unit in "ghbdtn".chars() {
                    let p = plan(engine, &unit.to_string(), 1, 1);
                    assert_eq!(kr_commit_compatibility(engine, p.plan_id, 0), OK);
                }
                let p = plan(engine, " ", 1, 1);
                assert_eq!((p.operation, p.delete_units, p.disposition), (1, 7, 0));
                assert_eq!(kr_commit_compatibility(engine, p.plan_id, 1), OK);
            }
            kr_engine_free(engine);
        }
    }
}
