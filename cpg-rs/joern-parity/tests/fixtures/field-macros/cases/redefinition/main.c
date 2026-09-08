struct Item { int len; int code; };
#define Len len
#define READ(p) ((p)->Len)
int first(struct Item *p) { return READ(p); }
#undef Len
#define Len code
int second(struct Item *p) { return READ(p); }
