#define WIDTH "ll"
#define FORMAT "%lld"
#define JOINED "macro" "replacement"
#define WRAPPED() "function" "replacement"
void consume(char *value);
char *plain(int condition) { return condition ? "left" "right" : "fallback"; }
char *lua_literal(int n, int minimum) {
    { const char *format = (n == minimum) /* corner case */
        ? "0x%" WIDTH "x" /* hexadecimal */
        : FORMAT;
      consume(format);
      return format;
    }
}
char *literal_return(void) { return "first" "second"; }
char *commented(int condition) { return condition ? "first" /* join */ "second" : "safe"; }
void argument(void) { consume("first" "second"); }
void parenthesized(void) { consume(("first" "second")); }
char *wide(int condition) { return condition ? L"wide" L"strings" : L"safe"; }
char *utf8(int condition) { return condition ? u8"utf8" u8"strings" : u8"safe"; }
char *utf16(int condition) { return condition ? u"utf16" u"strings" : u"safe"; }
char *utf32(int condition) { return condition ? U"utf32" U"strings" : U"safe"; }
char *mixed_prefix(int condition) { return condition ? "plain" L"wide" : L"safe"; }
char *macro_replacement(void) { return JOINED; }
char *function_replacement(void) { return WRAPPED(); }
char *macro_condition(int condition) { return condition ? JOINED : WRAPPED(); }
void multiline(void) {
    consume("first"
            "second");
    consume("following");
}
#define WIDE_JOIN L"wide" L"joined"
#define MIXED_JOIN "narrow" L"joined"
#define ESCAPED_JOIN "one\\" "two\"" "\nthree"
#define COMMENT_JOIN "left" /* separator */ "right"
#define PAREN_JOIN ("left" "right")
#define CALL_JOIN consume("left" "right")
char *wide_macro(void) { return WIDE_JOIN; }
char *mixed_macro(void) { return MIXED_JOIN; }
char *escaped_macro(void) { return ESCAPED_JOIN; }
char *comment_macro(void) { return COMMENT_JOIN; }
char *parenthesized_macro(void) { return PAREN_JOIN; }
void call_macro(void) { CALL_JOIN; }
