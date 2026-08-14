#define LEVEL 3
#define ENABLED 1
#define VERSION_AT_LEAST(x, y) (((x) * 10 + (y)) >= 32)

int preprocessor(void) {
#if LEVEL >= 3 && defined(ENABLED)
    live_primary();
#elif LEVEL == 2
    dead_elif();
#else
    dead_else();
#endif
#if VERSION_AT_LEAST(3, 2)
    live_function_macro();
#endif
#if UNDEFINED_NAME
    dead_undefined();
#endif
#if !defined(MISSING)
    live_not_defined();
#endif
    return 0;
}
