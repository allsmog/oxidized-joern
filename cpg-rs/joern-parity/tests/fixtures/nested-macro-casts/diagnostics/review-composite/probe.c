union U;
#define CAST(p) ((int (*)[sizeof((union U*)p)])p)
void *composite(void *p) { return CAST(p); }
