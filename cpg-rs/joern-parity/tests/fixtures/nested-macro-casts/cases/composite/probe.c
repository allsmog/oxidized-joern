#define CAST(p) ((int (*)[sizeof((union U*)p)])p)
#define DOUBLE(p) ((int (**)[sizeof((union U*)p)])p)
#define MATRIX(p) ((int (*)[2][sizeof((union U*)p)])p)
#define POINTER(p) ((union U **)p)
#define QUALIFIED(p) ((int (*const *)[sizeof((union U*)p)])p)
#define VALUE(p) ((union U *)(void *)p)
void *composite(void *p) { return CAST(p); }
void *double_pointer(void *p) { return DOUBLE(p); }
void *matrix(void *p) { return MATRIX(p); }
void *nested_value(void *p) { return VALUE(p); }
void *pointer(void *p) { return POINTER(p); }
void *qualified(void *p) { return QUALIFIED(p); }
