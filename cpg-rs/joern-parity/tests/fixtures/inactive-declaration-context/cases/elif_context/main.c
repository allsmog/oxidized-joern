#define N 3
#if 0
int a = N;
#elif 1
#undef N
#define N 4
int b = N;
#else
int c = N;
#endif
