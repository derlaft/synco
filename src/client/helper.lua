ready = false
seto = false

function on_pause_change(name, paused)
  if paused then
    set_ready(false)
  end
  set_seto(seto)
  on_seek(false)
end
mp.observe_property("pause", "bool", on_pause_change)

function on_seek(event)
  print("seek", mp.get_property_native("time-pos"))
end
mp.register_event("seek", on_seek)

function show_ready()
  if ready then
    mp.osd_message("ready")
  else 
    mp.osd_message("not ready")
  end
end

function toggle_ready()
  set_ready(not ready)
end
mp.add_forced_key_binding("F1", toggle_ready)

function set_ready(is_ready)
  ready = is_ready
  show_ready()
  print("ready", is_ready)

  if not ready then
    mp.set_property_bool("pause", true)
  end
end

function on_seto(value)
  set_seto(value == "true")
end
mp.register_script_message("seto", on_seto)

function set_seto(value) 
  seto = value

  stahp = not (ready and seto)
  mp.set_property_bool("pause", stahp)

  if not seto then
    set_ready(false)
  end

  if not stahp then
    mp.osd_message("go")
  end
end

set_ready(false) -- so it's paused on start
