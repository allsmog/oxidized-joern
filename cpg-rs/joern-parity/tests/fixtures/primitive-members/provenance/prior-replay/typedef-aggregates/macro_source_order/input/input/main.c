#define LIMIT 2
typedef struct { char first[LIMIT]; } First;
#undef LIMIT
#define LIMIT 5
typedef union { char second[LIMIT]; } Second;
