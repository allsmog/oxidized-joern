#define LUA_CORE
/*
** $Id: lcode.c $
** Code generator for Lua
** See Copyright Notice in lua.h
*/

#define lcode_c
#define LUA_CORE

#include "lprefix.h"


#include <float.h>
#include <limits.h>
#include <math.h>
#include <stdlib.h>

#include "lua.h"

#include "lcode.h"
#include "ldebug.h"
#include "ldo.h"
#include "lgc.h"
#include "llex.h"
#include "lmem.h"
#include "lobject.h"
#include "lopcodes.h"
#include "lparser.h"
#include "lstring.h"
#include "ltable.h"
#include "lvm.h"

#define ADD(a,b) l_castU2S(l_castS2U(a)+l_castS2U(b))
int direct(lua_Integer i){return l_castS2U(i);}
int add(lua_Integer i,lua_Integer j){return ADD(i,j);}
