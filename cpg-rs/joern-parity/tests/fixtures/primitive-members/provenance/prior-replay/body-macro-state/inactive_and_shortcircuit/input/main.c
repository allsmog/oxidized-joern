#define FLAG 1
#if 0
#if FLAG
#endif
#endif
#if 0 && FLAG
#endif
#undef FLAG
#define FLAG 2
int choose(void) { return FLAG; }
