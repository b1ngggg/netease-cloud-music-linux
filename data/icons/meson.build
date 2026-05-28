application_id = 'io.github.b1ngggg.netease_cloud_music_linux.cloudplayer'
runtime_icon_id = 'cloudplayer-current'

scalable_dir = join_paths('hicolor', 'scalable', 'apps')
install_data(
  join_paths(scalable_dir, ('@0@.svg').format(application_id)),
  install_dir: join_paths(get_option('datadir'), 'icons', scalable_dir)
)
install_data(
  join_paths(scalable_dir, ('@0@.svg').format(application_id)),
  rename: ('@0@.svg').format(runtime_icon_id),
  install_dir: join_paths(get_option('datadir'), 'icons', scalable_dir)
)

symbolic_dir = join_paths('hicolor', 'symbolic', 'apps')
install_data(
  join_paths(symbolic_dir, ('@0@-symbolic.svg').format(application_id)),
  install_dir: join_paths(get_option('datadir'), 'icons', symbolic_dir)
)
install_data(
  join_paths(symbolic_dir, ('@0@-symbolic.svg').format(application_id)),
  rename: ('@0@-symbolic.svg').format(runtime_icon_id),
  install_dir: join_paths(get_option('datadir'), 'icons', symbolic_dir)
)
