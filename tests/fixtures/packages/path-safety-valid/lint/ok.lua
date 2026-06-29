function register(lint)
  lint:rule({
    id = "path-safety/noop",
    title = "Path safety no-op",
    help = "No-op rule used to keep the path safety lint fixture registered.",
    handler = "check_package",
  })
end

function check_package(package, target)
  return {}
end
