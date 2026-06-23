function register(lint)
  lint:rule({
    id = "path-safety/noop",
    title = "Path safety no-op",
    help = "No-op rule used to keep the path safety lint fixture registered.",
    handler = "check_workspace",
  })
end

function check_workspace(workspace, target)
  return {}
end
