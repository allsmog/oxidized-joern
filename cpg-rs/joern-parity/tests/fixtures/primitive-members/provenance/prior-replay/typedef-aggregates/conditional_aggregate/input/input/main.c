#if 1
typedef struct { char a[2]; } Active;
#else
typedef union { char b[3]; } Inactive;
#endif
