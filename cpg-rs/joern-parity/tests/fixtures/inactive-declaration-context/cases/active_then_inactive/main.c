#define N 3
#if 1
#undef N
#define N 4
int active = N;
#else
int inactive = N;
#endif
int after = N;
