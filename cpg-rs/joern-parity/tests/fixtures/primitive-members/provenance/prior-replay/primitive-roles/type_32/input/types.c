_Bool identity(_Bool value) { _Bool local = value; return local; }
_Bool declared(_Bool value);
_Bool call_definition(_Bool value) { return identity(value); }
_Bool call_prototype(_Bool value) { return declared(value); }
