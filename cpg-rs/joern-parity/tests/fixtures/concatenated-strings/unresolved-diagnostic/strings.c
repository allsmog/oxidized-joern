char *plain(int condition) { return condition ? "left" "right" : "fallback"; }
char *unresolved(int condition) {
    { const char *format = condition ? "0x%" UNKNOWN_WIDTH "x" : UNKNOWN_FORMAT; return format; }
}
#define WIDTH "ll"
#define FORMAT "%lld"
char *defined(int condition) { return condition ? "0x%" WIDTH "x" : FORMAT; }
char *literal_return(void) { return "first" "second"; }
char *commented(int condition) { return condition ? "first" /* join */ "second" : "safe"; }
void consume(char *value);
void argument(void) { consume("first" "second"); }
