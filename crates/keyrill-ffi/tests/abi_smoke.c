// Drives the C ABI the way a native adapter does, against an in-memory document.
// The document is a run of committed text followed by the owned composition, with the caret
// at the end. Every write is checked against the plan's expected text before it is made.
#include "keyrill.h"

#include <stdio.h>
#include <string.h>

typedef struct {
  uint16_t text[512];
  uint32_t len;
  uint32_t mark_len; // trailing units of text that form the owned composition
} document;

static int failures;

static void fail(const char *what) {
  fprintf(stderr, "abi_smoke: %s\n", what);
  failures++;
}

static uint32_t apply(document *doc, const kr_plan *plan) {
  if (plan->expected_len > KR_TEXT_CAPACITY || plan->mark_len > KR_TEXT_CAPACITY)
    return KR_REFUSED_BEFORE_WRITE;
  // The composition must hold exactly what the engine expects
  uint32_t start = doc->len - doc->mark_len;
  if (plan->expected_len != doc->mark_len ||
      memcmp(plan->expected, doc->text + start, doc->mark_len * sizeof(uint16_t)) != 0)
    return KR_REFUSED_BEFORE_WRITE;
  if (start + plan->mark_len > sizeof doc->text / sizeof doc->text[0])
    return KR_REFUSED_BEFORE_WRITE;

  memcpy(doc->text + start, plan->mark, plan->mark_len * sizeof(uint16_t));
  doc->len = start + plan->mark_len;
  // MARK keeps the new text owned; FINISH commits it and ends the composition
  doc->mark_len = plan->action == KR_MARK ? plan->mark_len : 0;
  return KR_APPLIED_VERIFIED;
}

static void key(kr_engine *engine, document *doc, uint32_t role, uint32_t mode, uint16_t unit) {
  uint32_t context =
      doc->len == 0 || doc->text[doc->len - 1] == ' ' ? KR_CTX_BOUNDARY : KR_CTX_INELIGIBLE;
  kr_event event = {role, mode, context, role == KR_TEXT, {unit}};
  for (int attempt = 0; attempt < 2; attempt++) {
    kr_plan plan;
    if (kr_plan_event(engine, &event, &plan) != KR_OK) {
      fail("kr_plan_event refused an event");
      return;
    }
    uint32_t outcome = KR_APPLIED_VERIFIED;
    if (plan.action == KR_MARK || plan.action == KR_FINISH)
      outcome = apply(doc, &plan);
    else if (plan.action != KR_PASS && plan.action != KR_UNAVAILABLE)
      outcome = KR_REFUSED_BEFORE_WRITE;
    if (kr_commit(engine, plan.plan_id, outcome) != KR_OK || outcome != KR_APPLIED_VERIFIED) {
      fail("a plan did not match the document");
      return;
    }
    // FINISH + REPLAN wrote only the previous composition; the same key is planned again
    if (plan.action == KR_FINISH && plan.disposition == KR_KEY_REPLAN)
      continue;
    if (plan.disposition == KR_KEY_PASS && role == KR_TEXT) {
      if (doc->mark_len) {
        fail("a passed key would land inside the composition");
        return;
      }
      doc->text[doc->len++] = unit;
    }
    return;
  }
  fail("a second REPLAN is a protocol error");
}

static void type(kr_engine *engine, document *doc, uint32_t mode, const char *utf8) {
  // Only ASCII and two-byte Cyrillic UTF-8 occur in these scenarios
  for (const unsigned char *p = (const unsigned char *)utf8; *p;) {
    uint16_t unit = *p < 0x80 ? *p++ : (uint16_t)(((p[0] & 0x1f) << 6) | (p[1] & 0x3f));
    if (unit >= 0x80)
      p += 2;
    key(engine, doc, KR_TEXT, mode, unit);
  }
}

static void expect(const document *doc, const char *utf8, int committed, const char *scenario) {
  uint16_t want[512];
  uint32_t len = 0;
  for (const unsigned char *p = (const unsigned char *)utf8; *p; len++) {
    want[len] = *p < 0x80 ? *p : (uint16_t)(((p[0] & 0x1f) << 6) | (p[1] & 0x3f));
    p += *p < 0x80 ? 1 : 2;
  }
  if ((committed && doc->mark_len) || doc->len != len ||
      memcmp(doc->text, want, len * sizeof(uint16_t)) != 0)
    fail(scenario);
}

static void scenario(uint32_t keyboard, uint32_t mode, const char *typed, const char *shown,
                     uint32_t last, const char *result, const char *name) {
  kr_engine *engine = NULL;
  if (kr_engine_new(KR_ABI_VERSION, &engine) != KR_OK || !engine ||
      kr_set_automatic_policy(engine, KR_POLICY_FIXTURE) != KR_OK ||
      kr_set_keyboard(engine, keyboard) != KR_OK) {
    fail("engine setup");
    kr_engine_free(engine);
    return;
  }
  document doc = {{0}, 0, 0};
  type(engine, &doc, mode, typed);
  expect(&doc, shown, 0, name);
  key(engine, &doc, last, mode, 0);
  if (doc.mark_len)
    key(engine, &doc, KR_FINISH_REQUEST, mode, 0);
  expect(&doc, result, 1, name);
  kr_engine_free(engine);
}

// The default dictionary and a learned list under a home that doesn't exist count as empty
static void default_dictionary(void) {
#ifdef _WIN32
  static const char home[] = "C:\\keyrill-absent-home";
#else
  static const char home[] = "/keyrill-absent-home";
#endif
  kr_vocabulary *vocabulary = NULL;
  kr_engine *engine = NULL;
  uint32_t observation = UINT32_MAX;
  if (kr_vocabulary_new(KR_ABI_VERSION, &vocabulary) != KR_OK || !vocabulary ||
      kr_engine_new(KR_ABI_VERSION, &engine) != KR_OK || !engine)
    fail("vocabulary setup");
  else if (kr_vocabulary_configure_default(vocabulary, KR_EN, (const uint8_t *)"home", 4) !=
           KR_PATH)
    fail("a relative home was accepted");
  else if (kr_vocabulary_configure_default(vocabulary, KR_EN, (const uint8_t *)home,
                                           sizeof home - 1) != KR_OK ||
           kr_vocabulary_observe(vocabulary, KR_EN, &observation) != KR_OK ||
           observation != KR_SOURCE_PUBLISHED || kr_adopt_vocabulary(engine, vocabulary) != KR_OK)
    fail("a missing default dictionary was not published empty");
  else if (kr_vocabulary_configure(vocabulary, KR_LEARNED + KR_LEARNED_SOURCES - 1,
                                   (const uint8_t *)home, sizeof home - 1,
                                   KR_SOURCE_MISSING_IS_EMPTY | KR_SOURCE_EXCLUDED) != KR_OK ||
           kr_vocabulary_observe(vocabulary, KR_LEARNED + KR_LEARNED_SOURCES - 1, &observation) !=
               KR_OK ||
           observation != KR_SOURCE_PUBLISHED || kr_adopt_vocabulary(engine, vocabulary) != KR_OK)
    fail("a missing learned list was not published empty");
  kr_engine_free(engine);
  kr_vocabulary_free(vocabulary);
}

int main(void) {
  if (kr_abi_version() != KR_ABI_VERSION)
    fail("library and header versions differ");
  kr_engine *rejected = NULL;
  if (kr_engine_new(KR_ABI_VERSION + 1, &rejected) != KR_VERSION || rejected)
    fail("a different ABI version was accepted");

  const uint32_t apple = KR_KEYBOARD_APPLE, pc = KR_KEYBOARD_PC;
  scenario(apple, KR_EN, "ghbdtn ", "привет ", KR_FINISH_REQUEST, "привет ",
           "EN mode: ghbdtn + Space");
  scenario(apple, KR_RU, "руддщ ", "hello ", KR_FINISH_REQUEST, "hello ", "RU mode: руддщ + Space");
  scenario(apple, KR_EN, "ghbdtn ", "привет ", KR_ESCAPE, "ghbdtn ",
           "Escape restores the typed word");
  scenario(apple, KR_EN, "test ", "test ", KR_FINISH_REQUEST, "test ",
           "an unlisted word stays as typed");
  scenario(apple, KR_RU, "ёлка", "ёлка", KR_MANUAL, "\\krf", "Apple keyboard: ё is backslash");
  scenario(pc, KR_RU, "ёлка", "ёлка", KR_MANUAL, "`krf", "PC keyboard: ё is backtick");
  scenario(pc, KR_EN, "`krf", "`krf", KR_MANUAL, "ёлка", "EN mode: the PC ё key converts");
  scenario(apple, KR_EN, "[jhjij", "[jhjij", KR_MANUAL, "хорошо",
           "EN mode: keys for Russian letters convert");
  default_dictionary();

  if (failures)
    return 1;
  puts("abi_smoke: 8 scenarios and the default dictionary passed");
  return 0;
}
