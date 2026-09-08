struct Item { int len; int code; };
#define Len len
#define GET(p) ((p)->Len)
#define READ(p) GET(p)
int read(struct Item *p) { return READ(p); }
