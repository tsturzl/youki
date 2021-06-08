$script = <<-SCRIPT
su - vagrant -c 'curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- --default-toolchain stable -y'
curl -O https://dl.google.com/go/go1.16.5.linux-amd64.tar.gz
tar -C /usr/local -xzf go1.16.5.linux-amd64.tar.gz
export PATH=$PATH:/usr/local/go/bin
echo '\nexport PATH=$PATH:/usr/local/go/bin' > /etc/profile
SCRIPT

Vagrant.configure("2") do |config|
  config.vm.define "ubuntu" do |ubuntu|
    ubuntu.vm.box = "ubuntu/focal64"
    ubuntu.vm.provision "shell", inline: $script
    ubuntu.vm.synced_folder "./", "/srv/youki"
  end

  config.vm.define "fedora" do |fedora|
    fedora.vm.box = "generic/fedora33"
    fedora.vm.provision "shell", inline: $script
    fedora.vm.synced_folder "./", "/srv/youki"
  end

  config.vm.define "alpine" do |alpine|
    alpine.vm.box = "generic/alpine313"
    alpine.vm.provision "shell", inline: $script
    alpine.vm.synced_folder "./", "/srv/youki"
  end
end
