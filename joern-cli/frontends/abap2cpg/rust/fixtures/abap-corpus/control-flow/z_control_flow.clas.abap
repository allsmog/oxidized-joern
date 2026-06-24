* Coverage fixture: every statement below must classify to a known kind so
* the CLI prints no "abapastgen: N unclassified statement(s)" summary.
* Statements deliberately omitted because the classifier maps them to Unknown:
*   - PUBLIC SECTION. (section markers are not yet classified)
*   - WRITE / MESSAGE / COMMIT WORK (bare keywords with no '=' or method call)
CLASS z_control_flow DEFINITION PUBLIC.
  METHODS run.
ENDCLASS.

CLASS z_control_flow IMPLEMENTATION.
  METHOD run.
    DATA lv_x TYPE i.
    DATA lv_y TYPE i.
    lv_x = 1.
    MOVE 2 TO lv_y.

    IF lv_x = 1.
      lv_x = 2.
    ELSEIF lv_x = 2.
      lv_x = 3.
    ELSE.
      lv_x = 4.
    ENDIF.

    CASE lv_x.
      WHEN 1.
        lv_x = 0.
      WHEN OTHERS.
        lv_x = 9.
    ENDCASE.

    WHILE lv_x < 10.
      lv_x = lv_x + 1.
      CHECK lv_x > 0.
      CONTINUE.
    ENDWHILE.

    DO 5 TIMES.
      lv_x = lv_x - 1.
      EXIT.
    ENDDO.

    LOOP AT lt_tab INTO ls_row.
      lv_x = lv_x + 1.
    ENDLOOP.

    TRY.
      lv_x = 1.
    CATCH cx_root INTO lx_err.
      RAISE EXCEPTION TYPE cx_root.
    ENDTRY.

    RETURN.
  ENDMETHOD.
ENDCLASS.
