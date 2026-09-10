typedef long long lua_Integer;
typedef unsigned long long lua_Unsigned;
#define cast(t, exp) ((t)(exp))
#define l_castS2U(i) ((lua_Unsigned)(i))
#define l_castU2S(i) ((lua_Integer)(i))
#define intop(op,v1,v2) l_castU2S(l_castS2U(v1) op l_castS2U(v2))
lua_Integer add(lua_Integer v1,lua_Integer v2) { return intop(+, v1, v2); }
lua_Integer right(lua_Integer x,lua_Integer y) { return intop(>>, x, -y); }
lua_Integer left(lua_Integer x,lua_Integer y) { return intop(<<, x, y); }
