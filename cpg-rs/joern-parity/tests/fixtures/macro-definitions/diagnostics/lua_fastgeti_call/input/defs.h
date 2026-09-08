typedef struct Table { unsigned int alimit; int *array; } Table;
int ttistable(Table *table);
unsigned int l_castS2U(int key);
Table *hvalue(Table *table);
int *luaH_getint(Table *table, int key);
int isempty(int *slot);
#define NULL ((void *)0)
#define luaV_fastgeti(L,t,k,slot) \
  (!ttistable(t)  \
   ? (slot = NULL, 0)  /* not a table; 'slot' is NULL and result is 0 */  \
   : (slot = (l_castS2U(k) - 1u < hvalue(t)->alimit) \
              ? &hvalue(t)->array[k - 1] : luaH_getint(hvalue(t), k), \
      !isempty(slot)))  /* result not empty? */
