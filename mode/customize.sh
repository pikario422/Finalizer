Init(){
sh $MODPATH/vtools/init_vtools.sh $(realpath $MODPATH/module.prop)
}
enforce_install_from_magisk_app(){
echo "欢迎使用finalizer"
}
Init
enforce_install_from_magisk_app
