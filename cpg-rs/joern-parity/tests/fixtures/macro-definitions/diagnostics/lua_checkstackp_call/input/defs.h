typedef long ptrdiff_t;
ptrdiff_t savestack(int L, int p);
int restorestack(int L, ptrdiff_t offset);
#define luaD_checkstackaux(L,n,pre,pos) do { pre; pos; } while (0)
#define checkstackp(L,n,p)  \
  luaD_checkstackaux(L, n, \
    ptrdiff_t t__ = savestack(L, p),  /* save 'p' */ \
    p = restorestack(L, t__))  /* 'pos' part: restore 'p' */
