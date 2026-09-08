#define LUA_CORE
#include "lua.h"
#include "llimits.h"
#define ADD(a,b) l_castU2S(l_castS2U(a)+l_castS2U(b))
int direct(lua_Integer i){return l_castS2U(i);}
int add(lua_Integer i,lua_Integer j){return ADD(i,j);}
